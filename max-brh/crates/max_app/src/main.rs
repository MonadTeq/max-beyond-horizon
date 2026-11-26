//! `max-brh` — M.A.X. client CLI (the crate/library is `max_app`).
//!
//! Modes:
//!
//! * without flags (or `--ui`) — the interactive menu (create/select a game) and
//!   a session in an SDL3 window: this is the default mode, the `sdl` feature is part of
//!   default (`sdl-from-source`), i.e. `cargo run -p max_app` immediately opens
//!   the game;
//! * `--demo <save> [--turns N]` — headless: V70 save → N ticks → digest;
//! * `--new-game` — a local match on the planet map + a frame in PPM (`--out`);
//! * `--host [--listen <ip>] [--host-port <port>]` — P2P host: the ENet server
//!   and the server-side lobby 28…34 (`PLAN-host-mode.md` §5 H2, headless);
//! * `--connect <host:port>` (without `--join`) — the direct guest of that host;
//! * `--connect <horizon> --join <game> [--sdl]` — Horizon + ENet session;
//!   without `--sdl` the loop is headless, with `--sdl` — an SDL3 window (feature `sdl`).
//!
//! All game logic is in the library (`max_app`); here there is only argument
//! parsing and the SDL glue. Examples — in the crate's `README.md`.

// Windows: GUI builds (`--features sdl*`) — the `windows` subsystem, so
// double-clicking `max-brh.exe` does not open a console. A console build of the client without
// `sdl` remains console: `--demo`/`--new-game`/`--connect` print to the terminal's
// stdout and CI. `sdl-vcpkg` and `sdl-from-source` enable `sdl` (see
// Cargo.toml), so one cfg is enough for all windowed features.
#![cfg_attr(feature = "sdl", windows_subsystem = "windows")]

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use max_app::assets::AssetsReport;
#[cfg(feature = "sdl")]
use max_app::audio::AudioPlayer;
use max_app::host::{
    HostRunOptions, HostSessionSettings, SessionPolicy, DEFAULT_HOST_LOBBY_TIMEOUT,
    DEFAULT_MIN_PLAYERS,
};
#[cfg(feature = "sdl")]
use max_app::local::LocalSetup;
#[cfg(feature = "sdl")]
use max_app::music::{MusicDirector, MusicScene};
#[cfg(feature = "sdl")]
use max_app::net::run_network_with_sink;
#[cfg(feature = "sdl")]
use max_app::net::NetworkDispatcher;
use max_app::net::{
    run_network, DirectEndpoint, JoinRetryPolicy, NetworkOptions, NetworkReport, ReconnectPolicy,
};
#[cfg(feature = "sdl")]
use max_app::qa::{RecordContext, Recorder};
#[cfg(feature = "sdl")]
use max_app::story::{StoryContinuation, StoryKind, StoryScreen};
#[cfg(feature = "sdl")]
use max_app::subtitles::{SubtitleTrack, DEFAULT_LANGUAGE};
#[cfg(feature = "sdl")]
use max_app::ui::{
    handle_session_input, HorizonMenuClient, InputEvent, Key, Screen, ScreenView, SessionRequest,
    Transition, UiContext,
};
use max_app::{write_ppm, App, AppConfig, AppMode, NewGameConfig, NewGameReport};
use max_core::enums::{Planet, TeamClan};
use max_core::setup::ResourceLevel;
use max_net::{DEFAULT_HEARTBEAT_PERIOD, DEFAULT_HOST_PORT};
#[cfg(feature = "sdl")]
use max_render::sdl_window::{WindowInput, WindowKey};

const DEFAULT_SAVE: &str = "max-original/SAVE1.CAM";
const DEFAULT_GRPC_PORT: u16 = 8082;

/// What to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Demo,
    NewGame,
    Connect,
    /// P2P host: ENet server + server-side lobby (`PLAN-host-mode.md` §5 H2)
    /// and the host's own lockstep match (H3).
    Host,
    Ui,
}

/// Parsed CLI arguments.
#[derive(Debug)]
struct Cli {
    mode: Mode,
    config: AppConfig,
    save: Option<PathBuf>,
    turns: Option<u32>,
    game_id: Option<String>,
    password: Option<String>,
    /// `--host` was given (even if another mode flag came after it).
    host: bool,
    /// Raw `--connect` value: Horizon (with `--join`) or a direct host endpoint.
    connect: Option<String>,
    /// Direct P2P endpoint (`--connect` without `--join`).
    endpoint: Option<DirectEndpoint>,
    /// `--listen` address of the host ENet server.
    listen: Option<Ipv4Addr>,
    /// `--host-port` UDP port of the host ENet server.
    host_port: Option<u16>,
    /// `--min-players` READY threshold of the host lobby.
    min_players: Option<u32>,
    /// `--host-policy` session policy.
    host_policy: Option<SessionPolicy>,
    /// `--host-timeout` (5..=300 s): host-failure detection window (H4).
    host_timeout: Option<Duration>,
    /// `--host-heartbeat-ms` publishing period of numbered heartbeats (H4).
    host_heartbeat_ms: Option<u32>,
    sdl: bool,
    out: Option<PathBuf>,
    seed: Option<u32>,
    raw: Option<u8>,
    fuel: Option<u8>,
    gold: Option<u8>,
    derelicts: Option<u8>,
    /// Reconnect attempts on a disconnect (0 — disable).
    reconnect_attempts: Option<u32>,
    /// Timeout of one reconnect attempt in seconds.
    reconnect_timeout_secs: Option<u32>,
    /// How many first-connection attempts (Create game → Join → start).
    join_retry_attempts: Option<u32>,
    /// Timeout of one first-connection attempt in seconds.
    join_retry_timeout_secs: Option<u32>,
    /// Input window in the session PLAY phase, seconds (`0`/absent — headless: the turn
    /// closes immediately; see `max_app::net::NetworkOptions::turn_wait`).
    turn_wait_secs: Option<u32>,
    /// UI window scale: `0`/absent — fit to the desktop (item F).
    window_scale: Option<u32>,
    /// Open the window in borderless desktop fullscreen (`--fullscreen`; the
    /// runtime `Alt+Enter` toggle is handled by the SDL window layer).
    fullscreen: bool,
    /// Print the 60 Hz presentation statistics (`PRESENT: …`) to stderr.
    fps_stats: bool,
    /// Debug observation mode (`--no-fow`): draw the game without the fog of war
    /// (presentation only — the simulation state and the CRC are unchanged).
    no_fow: bool,
    /// Where to write the `.qa` manual game script (`--ui --record`).
    record: Option<PathBuf>,
}

impl Cli {
    fn parse() -> Result<Self> {
        let cli = Self::parse_from(std::env::args().skip(1))?;
        cli.ensure_gui_available()?;
        Ok(cli)
    }

    /// Argument parsing without checking for the SDL feature (it is checked by
    /// [`Cli::parse`]): this way CLI tests do not depend on the build's feature set.
    fn parse_from(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut cli = Self {
            mode: Mode::Ui,
            config: AppConfig::default(),
            save: None,
            turns: None,
            game_id: None,
            password: None,
            host: false,
            connect: None,
            endpoint: None,
            listen: None,
            host_port: None,
            min_players: None,
            host_policy: None,
            host_timeout: None,
            host_heartbeat_ms: None,
            sdl: false,
            out: None,
            seed: None,
            raw: None,
            fuel: None,
            gold: None,
            derelicts: None,
            reconnect_attempts: None,
            reconnect_timeout_secs: None,
            join_retry_attempts: None,
            join_retry_timeout_secs: None,
            turn_wait_secs: None,
            window_scale: None,
            fullscreen: false,
            fps_stats: false,
            no_fow: false,
            record: None,
        };
        let mut mode_set = false;
        let mut data_dir_set = false;
        let mut assets_dir_set = false;
        let mut patches_explicit = false;
        let mut args = args.into_iter();

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
                "--host" => {
                    cli.mode = Mode::Host;
                    cli.host = true;
                    mode_set = true;
                }
                "--listen" => {
                    let value = args.next().context("--listen требует IPv4-адрес")?;
                    cli.listen =
                        Some(value.parse().map_err(|_| {
                            anyhow!("--listen: ожидался IPv4-адрес, получено {value}")
                        })?);
                }
                "--host-port" => {
                    let value = parse_u32(&mut args, "--host-port")?;
                    cli.host_port = Some(u16::try_from(value).map_err(|_| {
                        anyhow!("--host-port должен быть 1..=65535, получено {value}")
                    })?);
                }
                "--min-players" => {
                    cli.min_players = Some(parse_u32(&mut args, "--min-players")?);
                }
                "--host-policy" => {
                    let value = args
                        .next()
                        .context("--host-policy требует pause|eliminate")?;
                    cli.host_policy = Some(SessionPolicy::parse(&value).ok_or_else(|| {
                        anyhow!("--host-policy: ожидалось pause|eliminate, получено {value}")
                    })?);
                }
                "--host-timeout" => {
                    let value = parse_u32(&mut args, "--host-timeout")?;
                    let timeout = Duration::from_secs(u64::from(value));
                    max_net::validate_host_timeout(timeout)
                        .map_err(|_| anyhow!("--host-timeout должен быть 5..=300 с, получено {value}"))?;
                    cli.host_timeout = Some(timeout);
                }
                "--host-heartbeat-ms" => {
                    let value = parse_u32(&mut args, "--host-heartbeat-ms")?;
                    if value == 0 || value > 60_000 {
                        bail!("--host-heartbeat-ms должен быть 1..=60000, получено {value}");
                    }
                    cli.host_heartbeat_ms = Some(value);
                }
                "--ui" => {
                    cli.mode = Mode::Ui;
                    mode_set = true;
                }
                "--connect" => {
                    cli.connect = Some(args.next().context("--connect требует <host[:port]>")?);
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
                "--reconnect-attempts" => {
                    cli.reconnect_attempts = Some(parse_u32(&mut args, "--reconnect-attempts")?);
                }
                "--reconnect-timeout" => {
                    cli.reconnect_timeout_secs = Some(parse_u32(&mut args, "--reconnect-timeout")?);
                }
                "--join-retry-attempts" => {
                    cli.join_retry_attempts = Some(parse_u32(&mut args, "--join-retry-attempts")?);
                }
                "--join-retry-timeout" => {
                    cli.join_retry_timeout_secs =
                        Some(parse_u32(&mut args, "--join-retry-timeout")?);
                }
                "--turn-wait" => {
                    cli.turn_wait_secs = Some(parse_u32(&mut args, "--turn-wait")?);
                }
                "--scale" => {
                    cli.window_scale = Some(parse_u32(&mut args, "--scale")?);
                }
                "--fullscreen" => {
                    cli.fullscreen = true;
                }
                "--fps" => {
                    cli.fps_stats = true;
                }
                // Debug observation mode: draw the world without the fog of war
                // (live AI observation; presentation only, the CRC is unchanged).
                "--no-fow" => {
                    cli.no_fow = true;
                }
                "--record" => {
                    cli.record = Some(PathBuf::from(
                        args.next().context("--record требует путь к .qa")?,
                    ));
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
                        args.next().context("--handshake-patches требует путь")?,
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

        // `--connect` is finalized here because `--join` may come after it:
        // with `--join` it is Horizon, without it — a direct P2P host endpoint
        // (`PLAN-host-mode.md` §5 H2).
        if cli.mode == Mode::Connect {
            let value = cli
                .connect
                .clone()
                .context("--connect требует <host[:port]>")?;
            if cli.game_id.is_some() {
                let (host, port) = parse_endpoint(&value)?;
                cli.config.horizon_host = host;
                cli.config.horizon_port = port;
            } else {
                let (host, port) = parse_direct_endpoint(&value)?;
                cli.endpoint = Some(DirectEndpoint { host, port });
            }
        }

        if cli.host && cli.mode != Mode::Host {
            bail!("--host не сочетается с другими режимами (--demo/--new-game/--connect/--ui)");
        }
        if cli.host && (cli.connect.is_some() || cli.game_id.is_some()) {
            bail!("--host не сочетается с --connect/--join (режим хоста поднимает сервер сам)");
        }
        if !cli.host
            && (cli.listen.is_some()
                || cli.host_port.is_some()
                || cli.min_players.is_some()
                || cli.host_policy.is_some()
                || cli.host_heartbeat_ms.is_some())
        {
            bail!("--listen/--host-port/--min-players/--host-policy/--host-heartbeat-ms требуют --host");
        }
        if let Some(min_players) = cli.min_players {
            if !(1..=4).contains(&min_players) {
                bail!("--min-players должен быть 1..=4, получено {min_players}");
            }
        }
        if cli.sdl && cli.mode != Mode::Connect {
            bail!("--sdl имеет смысл только с --connect <host> --join <game>");
        }
        if cli.sdl && cli.endpoint.is_some() {
            bail!("--sdl не поддерживается для прямого --connect (режим хоста, H2)");
        }
        if (cli.reconnect_attempts.is_some() || cli.reconnect_timeout_secs.is_some())
            && !matches!(cli.mode, Mode::Connect | Mode::Ui)
        {
            bail!("--reconnect-* имеет смысл только с --connect/--ui");
        }
        if (cli.join_retry_attempts.is_some() || cli.join_retry_timeout_secs.is_some())
            && !matches!(cli.mode, Mode::Connect | Mode::Ui)
        {
            bail!("--join-retry-* имеет смысл только с --connect/--ui");
        }
        if cli.record.is_some() && cli.mode != Mode::Ui {
            bail!(
                "--record работает только с GUI (запись ручной игры); \
                 без SDL доступен API max_app::qa::Recorder"
            );
        }

        cli.config.mode = match cli.mode {
            Mode::Demo => AppMode::Demo,
            Mode::NewGame => AppMode::NewGame,
            Mode::Connect => AppMode::Connect,
            Mode::Host => AppMode::Host,
            Mode::Ui => AppMode::Ui,
        };
        cli.config.save = cli.save.clone();
        cli.config.demo_turns = cli.turns.unwrap_or(5);

        // Default directories — from the repository root; if not set explicitly and
        // the relative path does not exist, try running from `max-brh/`.
        if !data_dir_set && !cli.config.max_res.exists() {
            if let Some(root) = repository_root() {
                cli.config = cli.config.with_data_dir(root.join("max-original"));
            }
        }
        // An explicit --patches disables the fallback: the client may deliberately point
        // to a nonexistent file, repeating the semantics of the C++ server (a missing
        // file gives an empty data_hash part in the handshake).
        if !assets_dir_set && !patches_explicit && !cli.config.patches_res.exists() {
            if let Some(root) = repository_root() {
                cli.config = cli.config.with_assets_dir(root.join("assets"));
            }
        }

        Ok(cli)
    }

    /// GUI (default mode, also `--ui`) requires the `sdl` feature — it is in
    /// default (`sdl-from-source`), so we get here only with a build using
    /// `--no-default-features`. Headless modes (`--demo`/`--new-game`/
    /// `--connect` without `--sdl`) do not require SDL.
    #[cfg(not(feature = "sdl"))]
    fn ensure_gui_available(&self) -> Result<()> {
        if self.mode == Mode::Ui {
            bail!(
                "GUI-режим (запуск без флагов или --ui) требует сборки с feature sdl \
                 (в default-features), а этот бинарь собран с --no-default-features: \
                 cargo run -p max_app (без --no-default-features)\n\
                 headless-режимы: --demo <save>, --new-game, --host, \
                 --connect <host> --join <game>"
            );
        }
        Ok(())
    }

    #[cfg(feature = "sdl")]
    fn ensure_gui_available(&self) -> Result<()> {
        Ok(())
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
        config.clans[self.config.team_slot as usize] = self.config.clan;

        config
    }
}

/// Log level from `MAX_LOG`/`RUST_LOG`.
///
/// `tracing-subscriber` is built without `env-filter`, so `RUST_LOG` by
/// itself does not work. For smoke logs (passing 52/54/45 and the relay)
/// we accept a simple level (`debug`, `trace`, …); `target=level` is also
/// accepted, but the target is ignored — the level is applied globally.
fn log_level() -> tracing_subscriber::filter::LevelFilter {
    for name in ["MAX_LOG", "RUST_LOG"] {
        let Ok(value) = std::env::var(name) else {
            continue;
        };
        let value = value.trim();
        if let Ok(level) = value.parse() {
            return level;
        }
        if let Some((_, level)) = value.rsplit_once('=') {
            if let Ok(level) = level.trim().parse() {
                return level;
            }
        }
    }
    tracing_subscriber::filter::LevelFilter::INFO
}

fn main() -> ExitCode {
    tracing_subscriber::fmt().with_max_level(log_level()).init();

    match Cli::parse().and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("max-brh: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    if cli.mode == Mode::Ui {
        return run_ui(&cli);
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("tokio runtime")?;

    runtime.block_on(async move {
        match cli.mode {
            Mode::Demo => run_demo(&cli),
            Mode::NewGame => run_new_game(&cli),
            Mode::Connect => run_connect(&cli).await,
            Mode::Host => run_host(&cli).await,
            Mode::Ui => unreachable!("--ui разобран выше"),
        }
    })
}

/// `--demo <save> [--turns N]`: headless run of a save.
fn run_demo(cli: &Cli) -> Result<()> {
    let explicit = cli.save.is_some() || cli.config.save.is_some();
    let mut save = cli
        .save
        .clone()
        .or_else(|| cli.config.save.clone())
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SAVE));

    // The default path is set from the repository root: we try the same file next to
    // MAX.RES (the data directory is already found). An explicitly given path is not substituted.
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

/// `--new-game`: local map generation, frame in PPM (`--out`).
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

    // As in the original: at the start of the match the first combat-capable unit of the player
    // with a sprite is selected — then the `--new-game` frame immediately shows the portrait,
    // statistics and description rather than empty panels.
    let selected = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state_ref().unit(id))
        .find(|unit| {
            unit.team == app.player_team()
                && unit.hits > 0
                && app
                    .assets()
                    .is_some_and(|assets| assets.sprite(unit.unit_type).is_some())
        })
        .map(|unit| unit.id);

    if let Some(id) = selected {
        app.hud_mut().select(Some(id));
    }

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
        app.state_ref().player_team()
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
    let mut reconnect = ReconnectPolicy::default();
    if let Some(attempts) = cli.reconnect_attempts {
        reconnect.attempts = attempts;
    }
    if let Some(seconds) = cli.reconnect_timeout_secs {
        reconnect.timeout = Duration::from_secs(u64::from(seconds));
    }
    let options = NetworkOptions {
        game_id: cli.game_id.clone().unwrap_or_default(),
        password: cli.password.clone(),
        endpoint: cli.endpoint.clone(),
        max_turns: cli.turns.unwrap_or(0),
        reconnect,
        join_retry: join_retry_policy(cli),
        turn_wait: Duration::from_secs(u64::from(cli.turn_wait_secs.unwrap_or(0))),
        host_timeout: cli.host_timeout.unwrap_or(max_net::DEFAULT_HOST_TIMEOUT),
        ..NetworkOptions::default()
    };

    let report = if cli.sdl {
        connect_interactive(
            &mut app,
            &options,
            &cli.config,
            cli.window_scale,
            cli.fullscreen,
        )
        .await?
    } else {
        run_network(&mut app, &options).await?
    };

    print_network(&report);

    Ok(())
}

/// `--host [--listen <ip>] [--host-port <port>] [--min-players <n>] [--host-policy <p>]`.
///
/// H3: the host runs the ENet server, the server-side lobby (28…34) **and**
/// plays its own team through the same lockstep driver as a guest; `--turns N`
/// limits the number of turns of that match (`0` — until the match ends).
async fn run_host(cli: &Cli) -> Result<()> {
    let game = cli.new_game_config();
    let options = HostRunOptions {
        bind_ip: cli.listen.unwrap_or(Ipv4Addr::UNSPECIFIED),
        port: cli.host_port.unwrap_or(DEFAULT_HOST_PORT),
        host_name: cli.config.player_name.clone(),
        host_team: cli.config.team_slot.min(3),
        min_players: cli
            .min_players
            .map_or(DEFAULT_MIN_PLAYERS, |value| value as usize),
        policy: cli.host_policy.unwrap_or_default(),
        session: HostSessionSettings {
            planet: game.planet,
            seed: game.seed,
            raw: game.raw.value(),
            fuel: game.fuel.value(),
            gold: game.gold.value(),
            derelicts: game.derelicts.value(),
            start_gold: game.start_gold,
        },
        lobby_timeout: DEFAULT_HOST_LOBBY_TIMEOUT,
        max_turns: cli.turns.unwrap_or(0),
        host_timeout: cli.host_timeout.unwrap_or(max_net::DEFAULT_HOST_TIMEOUT),
        heartbeat: Duration::from_millis(u64::from(
            cli.host_heartbeat_ms
                .unwrap_or(DEFAULT_HEARTBEAT_PERIOD.as_millis() as u32),
        )),
    };

    println!(
        "host: слушаю {}:{} (min_players={}, policy={}, turns={})",
        options.bind_ip, options.port, options.min_players, options.policy, options.max_turns
    );

    let report = max_app::host::run_host(&cli.config, &options).await?;
    let settings = report.lobby.settings.as_ref();
    println!(
        "host: лобби=34 игроков={} (готовы {}) host_node={} team_nodes={:?} rng_seed={} world={} policy={}",
        report.status.remote_players + 1,
        report.status.ready_players,
        report.lobby.host_node,
        report.lobby.team_nodes,
        settings.map_or(0, |settings| settings.rng_seed),
        settings.map_or_else(String::new, |settings| settings.world_name.to_string()),
        report.policy
    );

    let game = &report.game;
    println!(
        "host-game: team={} ticks={} commands={} turns={} barrier_timeouts={} desyncs={} sent=[{}] recv=[{}]",
        game.local_team.0,
        game.ticks,
        game.commands_applied,
        game.turns_finished,
        game.barrier_timeouts,
        game.desyncs,
        game.sent.to_line(),
        game.received.to_line()
    );
    println!("digest: {}", game.digest);
    println!("shared: {}", game.shared.to_line());
    // H5: one line per desync audit (23/24/49); the report file is written by
    // the host into the process directory.
    for audit in &game.audits {
        println!("host-audit: {}", audit.to_line());
    }

    Ok(())
}

/// First-connection retry policy from the CLI (`--join-retry-*`).
fn join_retry_policy(cli: &Cli) -> JoinRetryPolicy {
    let mut policy = JoinRetryPolicy::default();
    if let Some(attempts) = cli.join_retry_attempts {
        policy.attempts = attempts;
    }
    if let Some(seconds) = cli.join_retry_timeout_secs {
        policy.timeout = Duration::from_secs(u64::from(seconds));
    }
    policy
}

fn print_assets(report: Option<&AssetsReport>) {
    let Some(report) = report else { return };

    println!(
        "assets: MAX.RES={} PATCHES.RES={} world={} ({}; {}x{}) sprites={} flics={}",
        report.max_res_entries,
        report.patches_res_entries,
        report.world,
        report.world_source,
        report.world_size.0,
        report.world_size.1,
        report.sprite_count(),
        report.flics
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
        "network: game={} handshake={} lobby={} team={} ticks={} commands={} turns={} barrier_timeouts={} desyncs={} sent=[{}] recv=[{}] reconnects={} reconnect_attempts={} reconnect_pending_state={} join_attempts={} data_hash={}({})",
        report.game_id,
        report.handshake,
        report.lobby_complete,
        report.local_team.0,
        report.ticks,
        report.commands_applied,
        report.turns_finished,
        report.barrier_timeouts,
        report.desyncs,
        report.sent.to_line(),
        report.received.to_line(),
        report.reconnects,
        report.reconnect_attempts,
        report.reconnect_pending_state,
        report.join_attempts,
        &report.data_hash[..report.data_hash.len().min(8)],
        report.data_hash.len()
    );
    println!("digest: {}", report.digest);
    println!("shared: {}", report.shared.to_line());
    // H5: the desync audit round this guest took part in (if the CRC diverged).
    for audit in &report.audits {
        println!("audit: {}", audit.to_line());
    }
}

/// One SDL window input → the UI [`InputEvent`].
///
/// Left button — select/attack, right — move order/cancel (M4.3).
#[cfg(feature = "sdl")]
fn input_event(input: WindowInput) -> InputEvent {
    match input {
        WindowInput::Key(WindowKey::Escape) => InputEvent::Key(Key::Escape),
        WindowInput::Key(WindowKey::Enter) => InputEvent::Key(Key::Enter),
        WindowInput::Key(WindowKey::Tab) => InputEvent::Key(Key::Tab),
        WindowInput::Key(WindowKey::Backspace) => InputEvent::Key(Key::Backspace),
        WindowInput::Key(WindowKey::Up) => InputEvent::Key(Key::Up),
        WindowInput::Key(WindowKey::Down) => InputEvent::Key(Key::Down),
        WindowInput::Key(WindowKey::Left) => InputEvent::Key(Key::Left),
        WindowInput::Key(WindowKey::Right) => InputEvent::Key(Key::Right),
        WindowInput::Key(WindowKey::PageUp) => InputEvent::Key(Key::PageUp),
        WindowInput::Key(WindowKey::PageDown) => InputEvent::Key(Key::PageDown),
        WindowInput::Key(WindowKey::Char(character)) => InputEvent::Key(Key::Char(character)),
        WindowInput::Key(WindowKey::Alt(character)) => InputEvent::Key(Key::Alt(character)),
        WindowInput::Key(WindowKey::Other) => InputEvent::Key(Key::Other),
        WindowInput::MouseMove { x, y } => InputEvent::MouseMove(x, y),
        WindowInput::MouseClick { x, y } => InputEvent::MouseClick(x, y),
        WindowInput::MouseRelease { x, y } => InputEvent::MouseRelease(x, y),
        WindowInput::MouseRightClick { x, y } => InputEvent::RightClick(x, y),
    }
}

/// SDL3 window: library frame → texture → screen.
#[cfg(feature = "sdl")]
struct WindowSink {
    window: max_render::SdlWindow,
}

#[cfg(feature = "sdl")]
impl max_app::net::FrameSink for WindowSink {
    /// The window-only sink has no input: `--connect --sdl` without the HUD.
    fn poll_input(&mut self, _app: &mut App<NetworkDispatcher>) -> bool {
        true
    }

    fn render(&mut self, app: &mut App<NetworkDispatcher>) -> bool {
        app.update_visual_units(1.0 / 60.0);
        let buffer = app.render_frame();
        // One drawn frame of the press feedback: this sink bypasses `Screen::render`.
        app.advance_button_press();

        // Presentation only: a hidden window is not drawn, the lockstep loop
        // keeps running (barriers/timers are not tied to focus).
        if self.window.should_present() {
            if let Err(error) = self.window.present(&buffer) {
                eprintln!("SDL: {error}");
                return false;
            }
        }

        self.window.poll()
    }
}

/// Launch of `--connect --sdl`: the network loop draws frames into the window.
async fn connect_interactive(
    app: &mut App,
    options: &NetworkOptions,
    config: &AppConfig,
    scale: Option<u32>,
    fullscreen: bool,
) -> Result<NetworkReport> {
    #[cfg(feature = "sdl")]
    {
        let mut window = max_render::SdlWindow::new_scaled(
            "M.A.X. — max_app (R2.4)",
            config.screen_width,
            config.screen_height,
            normalized_window_scale(scale),
        )
        .map_err(|error| anyhow!("{error}"))?;

        if fullscreen {
            if let Err(error) = window.set_fullscreen(true) {
                eprintln!("SDL: полноэкранный режим недоступен ({error})");
            }
        }

        println!(
            "SDL: кадр {}x{}, окно {}x{} точек / {}x{} пикселей (DPI×{:.2}, масштаб {}, \
             fullscreen={}); Esc/закрытие — выход",
            config.screen_width,
            config.screen_height,
            window.size().0,
            window.size().1,
            window.output_size().0,
            window.output_size().1,
            window.dpi_scale(),
            window.frame_scale().scale(),
            window.is_fullscreen()
        );
        let mut sink = WindowSink { window };

        run_network_with_sink(app, options, Some(&mut sink)).await
    }

    #[cfg(not(feature = "sdl"))]
    {
        let _ = (app, options, config, scale, fullscreen);
        bail!(
            "--sdl требует сборки с feature sdl (входит в default-features): \
             cargo run -p max_app -- --connect ... --join ... --sdl"
        )
    }
}

// ---------------------------------------------------------------------------
// --ui: interactive menu in an SDL3 window
// ---------------------------------------------------------------------------

#[cfg(feature = "sdl")]
fn build_ui_runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("tokio runtime (UI)")
}

/// Loads the `Introduction` captions (C3).
///
/// The oracle reads the `INTROSUB` resource (an ASCIIZ file name) and opens the JSON
/// document next to the game data (`moviesubtitle.cpp:74-100`). Game directories without
/// the resource fall back to `<mve stem>.json` next to `MAX.RES` (the port's asset layout,
/// e.g. `maxint.json`). Every failure is a note for the status line/log — the movie simply
/// plays without captions.
#[cfg(feature = "sdl")]
fn load_intro_subtitles(
    assets: &max_app::Assets,
    data_dir: &std::path::Path,
    notes: &mut Vec<String>,
) -> Option<SubtitleTrack> {
    let mut failure = match SubtitleTrack::load(&assets.max_res, data_dir, DEFAULT_LANGUAGE) {
        Ok(track) => {
            println!(
                "UI: субтитры: {} — {} реплик из {}",
                track.language(),
                track.len(),
                track
                    .source()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default()
            );

            return Some(track);
        }
        Err(error) => error,
    };

    // Sidecar next to the movie (the oracle's asset directory layout): `maxint.json`.
    let sidecar = assets.intro_mve.as_ref().and_then(|path| {
        let candidate = path.with_extension("json");

        candidate.is_file().then_some(candidate)
    });

    if let Some(path) = sidecar {
        match SubtitleTrack::load_file(&path, DEFAULT_LANGUAGE) {
            Ok(track) => {
                println!(
                    "UI: субтитры: {} — {} реплик из {} (файл рядом с MAX.RES)",
                    track.language(),
                    track.len(),
                    path.display()
                );

                return Some(track);
            }
            Err(error) => failure = error,
        }
    }

    notes.push(format!("субтитры: {failure} — без субтитров"));

    None
}

/// Plays the story-screen music (`StoryBlock::music`, `menu.cpp:803-805`) and
/// returns the visible status line (fallback diagnostics or empty).
#[cfg(feature = "sdl")]
fn start_story_music(
    music: &mut MusicDirector,
    audio: &mut AudioPlayer,
    story: &mut StoryScreen,
) -> String {
    if let Some(tag) = story.music().map(str::to_string) {
        if !music.play_story(&tag, audio) {
            let reason = music
                .last_error()
                .map(str::to_string)
                .unwrap_or_else(|| "the track is unavailable".to_string());
            story.note_music_error(&tag, &reason);
            eprintln!("UI: story music: {tag} — {reason}");
        }
    }

    story.status_line()
}

/// Resumes the driver action kept aside when the story screen opened
/// (`menu.cpp:514-545`, `2096-2098`).
#[cfg(feature = "sdl")]
fn finish_story_screen(
    screen: &mut Screen,
    story_next: &mut Option<StoryContinuation>,
    active_mission: &mut Option<max_assets::MissionDescriptor>,
    app: &mut App,
    ctx: &mut UiContext,
    music: &mut MusicDirector,
) {
    music.end_story();

    match story_next.take() {
        Some(StoryContinuation::EnterSession) => {
            *screen = Screen::InSession {
                request: SessionRequest {
                    game_id: "local".to_string(),
                    password: None,
                },
            };
            ctx.status.clear();
        }
        Some(StoryContinuation::LeaveSession) | None => {
            *screen = Screen::MainMenu;
            *active_mission = None;
            // `menu.cpp:518-545`: a won campaign mission advances the progression at
            // the point the oracle returns to the main menu (after the story).
            app.leave_local_game();
            ctx.preferences = app.preferences().clone();
            ctx.status = "Left the session".to_string();
        }
    }
}

/// SDL UI loop: window events → [`InputEvent`] → [`Screen`] → frame.
#[cfg(feature = "sdl")]
fn run_ui(cli: &Cli) -> Result<()> {
    use max_render::buffer::RgbaBuffer;
    use max_render::sdl_window::SdlWindow;

    let config = &cli.config;
    let mut app = App::new(config.clone());

    match app.init_assets() {
        Ok(report) => {
            print_assets(Some(&report));
            println!("UI: ассеты загружены — карта рисуется настоящими цветами");
        }
        Err(error) => {
            tracing::warn!("UI: данные игры не загружены ({error:#}); карта будет упрощённой");
        }
    }
    // C++ `settings.json` (`FilePathGamePref`): the player settings survive
    // a restart. The QA driver does not read the file — there the values are default.
    if app.load_preferences_from_disk() {
        println!(
            "UI: настройки загружены из {}",
            app.preferences_path().display()
        );
    }
    // Debug observation of the AI (`--no-fow`): the fog of war is not drawn, so
    // the opponent plays "in peace" in front of the observer. Presentation only.
    if cli.no_fow {
        app.set_no_fow(true);
        println!("UI: режим отладки ИИ — туман войны отключён (--no-fow)");
    }

    let mut window = SdlWindow::new_scaled(
        "M.A.X. — Rust client (M4.2 HUD)",
        config.screen_width,
        config.screen_height,
        normalized_window_scale(cli.window_scale),
    )
    .map_err(|error| anyhow!("{error}"))?;
    if cli.fullscreen {
        // Presentation only: the 640×480 frame is unchanged, the geometry is
        // recomputed for the display (task 1.4.2).
        if let Err(error) = window.set_fullscreen(true) {
            eprintln!("UI: полноэкранный режим недоступен ({error}); окно остаётся оконным");
        }
    }
    let mut ctx = UiContext::new(
        Box::new(HorizonMenuClient::new(
            &config.horizon_host,
            config.horizon_port,
        )),
        config.screen_width,
        config.screen_height,
    );
    // `Introduction`: INTROFLC → maxint.mve next to MAX.RES (see Assets::load).
    ctx.intro_mve_path = app.assets().and_then(|assets| assets.intro_mve.clone());
    ctx.preferences = app.preferences().clone();
    // M6.2: menu sound. `with_default_backend` behind the `sound` feature brings up
    // rodio/cpal; without the feature (or without a device) — a silent no-op. The references to
    // files are taken from MAX.RES, the files themselves — next to MAX.RES.
    ctx.audio = AudioPlayer::with_default_backend();
    if let Some(assets) = app.assets() {
        let sound_dir = config
            .max_res
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_default();
        ctx.audio.load_from(&assets.max_res, sound_dir);
    }
    println!(
        "UI: звук: backend={} файлов={} ссылок={}",
        ctx.audio.backend_name(),
        ctx.audio.file_count(),
        ctx.audio.reference_count()
    );
    if let Some(error) = ctx.audio.errors().first() {
        eprintln!("UI: звук: {error}");
    }
    // C3/D2: the presentation media layers. Each one is optional — a missing movie,
    // soundtrack or subtitle file never blocks the game (owner's fallback condition),
    // but the reason is printed and kept for the menu status line (no silent fallback).
    let mut media_notes: Vec<String> = Vec::new();
    let data_dir = config
        .max_res
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    if let Some(assets) = app.assets() {
        ctx.intro_subtitles = load_intro_subtitles(assets, &data_dir, &mut media_notes);
    }
    if ctx.intro_mve_path.is_none() {
        media_notes.push("видео: maxint.mve нет рядом с MAX.RES — заглушка".to_string());
    }
    // Campaign stage 2 (`GameSetupMenu`): mission descriptors come from
    // `PATCHES.RES` (`MissionRegistry`), and the mission files (`SAVE1.CAM`, …) are
    // game data next to `MAX.RES` — not the save directory. A descriptor whose file
    // is absent stays visible in the list with a `[missing]` marker (owner
    // condition 5), so the menu never starts an empty world.
    let mut campaign_mission_count: Option<usize> = None;
    if let Some(assets) = app.assets() {
        let mission_dir = config
            .max_res
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_default();
        let registry = max_assets::MissionRegistry::from_archive(&assets.patches_res, &mission_dir);
        let missing: Vec<&String> = registry
            .warnings()
            .iter()
            .filter(|warning| warning.contains("not found") || warning.contains("cannot read"))
            .collect();

        for warning in &missing {
            eprintln!("UI: миссии: {warning}");
        }

        if !missing.is_empty() {
            let note = format!(
                "миссии: {} файл(ов) миссий не найдено — в списке помечены [missing]",
                missing.len()
            );
            // The mission list itself marks the rows `[missing]`; the note stays in
            // the client log and is not drawn over the menu (the status line is meant
            // for player-facing messages, not for launch diagnostics).
            eprintln!("UI: {note}");
        }

        // Campaign progression (stage 3a): the victory wrap-up needs the size of
        // the campaign mission list (`MissionManager::GetMissions(...).size()`).
        campaign_mission_count = Some(
            registry
                .missions(max_core::victory::MissionCategory::Campaign)
                .len(),
        );
        ctx.missions = Some(std::sync::Arc::new(registry));
    }
    if let Some(count) = campaign_mission_count {
        app.set_campaign_mission_count(count);
    }

    let mut music = MusicDirector::new();
    if ctx.audio.is_audible() {
        if ctx.audio.has_music(max_app::music::TRACK_MENU) {
            println!("UI: музыка: *.MSC найдены (disable_music учитывается)");
        } else {
            media_notes.push("музыка: файлы *.MSC не найдены — без музыки".to_string());
        }
    } else {
        media_notes.push(format!(
            "музыка: звуковой выход недоступен ({}) — без музыки",
            ctx.audio.backend_name()
        ));
    }
    if !media_notes.is_empty() {
        for note in &media_notes {
            eprintln!("UI: {note}");
        }

        // Launch diagnostics stay in the log: the GNW menu font has no Cyrillic and
        // draws `?`, and the original main screen has no technical status line.
    }
    let mut reported_music_errors = 0usize;
    // `main.cpp:39-46`: the intro movie is the first screen when the data is present.
    let mut screen = max_app::ui::startup_screen(
        ctx.intro_mve_path.clone(),
        ctx.intro_subtitles.clone(),
    );

    if let Screen::Introduction(intro) = &screen {
        println!(
            "UI: интро при старте — кадр {} (Esc — в меню)",
            intro.frame_index()
        );
    }
    let mut buffer = RgbaBuffer::new(config.screen_width, config.screen_height);
    let mut mouse: Option<(i32, i32)> = None;
    let mut runtime: Option<tokio::runtime::Runtime> = None;
    let mut pending: Option<SessionRequest> = None;
    // Campaign stage 3b: the descriptor of the loaded mission (presentation memory,
    // `MissionManager::GetMission`) and the driver action the story screen returns
    // to (`menu.cpp:497-512`, `2065-2071`).
    let mut active_mission: Option<max_assets::MissionDescriptor> = None;
    let mut story_next: Option<StoryContinuation> = None;
    // M-SP: the player name and the map directory for setup and planet minimaps.
    // `player_name` — from `settings` (`preferences.ini`), like C++ `OptionsMenu`
    // `SETUP`; the CLI `--player` remains the network client name (`config`).
    ctx.player_name = app.preferences().player_name.clone();
    ctx.world_dir = config
        .world_dir
        .clone()
        .or_else(|| config.max_res.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_default();
    let mut last_frame = std::time::Instant::now();
    // 1.4.2: presentation clock. The simulation advances in fixed 60 Hz steps
    // (`FixedStepClock`), the loop draws one frame per `FramePacer` period. A
    // 144 Hz display therefore still ticks the simulation at exactly 60 Hz;
    // `snapshot_bytes` does not depend on the refresh rate.
    let mut step_clock = max_app::presentation::FixedStepClock::new();
    let mut pacer = max_app::presentation::FramePacer::at_60hz();
    let mut frame_stats = max_app::presentation::FrameStats::default();
    let mut was_focused = window.is_focused();
    let mut logged_frames = 0u64;
    // The reconnect policy shared by `--connect` and `--ui`.
    let mut reconnect = ReconnectPolicy::default();
    if let Some(attempts) = cli.reconnect_attempts {
        reconnect.attempts = attempts;
    }
    if let Some(seconds) = cli.reconnect_timeout_secs {
        reconnect.timeout = Duration::from_secs(u64::from(seconds));
    }
    // Create game → InSession: the headless session warms up for 10–15 s, so the first
    // `SessionHandle::start` is repeated with backoff instead of failing into the menu.
    let join_retry = join_retry_policy(cli);

    println!(
        "UI: кадр {}x{}, окно {}x{} точек / {}x{} пикселей (DPI×{:.2}, масштаб {}, \
         дисплей {:.0} Гц, fullscreen={}); меню — мышь/стрелки, Enter; \
         в игре — LMB выбор/движение/меню, RMB выбор/контекст, клик по врагу — атака, End Turn; Esc — выход",
        config.screen_width,
        config.screen_height,
        window.size().0,
        window.size().1,
        window.output_size().0,
        window.output_size().1,
        window.dpi_scale(),
        window.frame_scale().scale(),
        window.display_refresh_rate(),
        window.is_fullscreen()
    );

    // QA layer 2.5: recording a manual game into a `.qa` script. We write the same
    // `InputEvent` (already in 640×480 frame pixels) that the screens receive;
    // the start — immediately, from the main menu, the finish — on exit.
    let mut recorder = match cli.record.as_ref() {
        Some(path) => {
            let recorder = Recorder::new(path.as_path())
                .with_context(|| format!("запись {}", path.display()))?;
            let actual = recorder
                .path()
                .map(|path| path.display().to_string())
                .unwrap_or_default();
            println!("RECORD: {actual} (кадр 640x480; ассерты/shot добавить вручную)");
            Some(recorder)
        }
        None => None,
    };

    loop {
        let frame_started = std::time::Instant::now();

        if let Some(recorder) = recorder.as_mut() {
            recorder.begin_frame();
        }

        let (running, events) = window.poll_input();

        if !running {
            break;
        }

        // 1.4.2 alt-tab gate: focus is presentation only. On regain the step
        // clock and the frame cadence restart from "now", so the time spent
        // away neither resets the match nor turns into a burst of ticks.
        let focused = window.is_focused();

        if focused && !was_focused {
            step_clock.reset();
            last_frame = std::time::Instant::now();
            pacer.resync(last_frame);
        }

        was_focused = focused;

        for event in events {
            let input = input_event(event);

            if std::env::var_os("MAX_DEBUG_INPUT").is_some() {
                let name = match &screen {
                    Screen::MainMenu => "MainMenu",
                    Screen::Preferences(_) => "Preferences",
                    Screen::LocalSetup(_) => "LocalSetup",
                    Screen::NewGameMenu { .. } => "NewGameMenu",
                    Screen::MissionSelect(_) => "MissionSelect",
                    Screen::InSession { .. } => "InSession",
                    Screen::GameOver => "GameOver",
                    Screen::MissionStory(_) => "MissionStory",
                    Screen::Introduction(_) => "Introduction",
                    Screen::Credits(_) => "Credits",
                    Screen::Help { .. } => "Help",
                    _ => "Other",
                };
                // Match modal windows: by these fields the live UI test
                // (`max-brh-cicd/scripts/ui-test-allocate.ps1`) understands that the extraction
                // allocation dialog opened/closed.
                let alloc = if app.alloc_menu().is_some() {
                    "alloc=open"
                } else {
                    "alloc=closed"
                };
                // X-FER (`TransferMenu`, `XFERPIC`) — for the live UI test.
                let transfer = if app.transfer_menu().is_some() {
                    "xfer=open"
                } else {
                    "xfer=closed"
                };
                // `BuildMenu` (`buildmenu`): the live construction test uses it
                // (`max-brh-cicd/scripts/ui-probe-build.ps1`) to see that the screen opened.
                let build = if app.build_menu().is_some() {
                    "build=open"
                } else {
                    "build=closed"
                };
                // Site selection mode (`ORDER_STATE_SELECT_SITE`): the live test
                // checks by this line that the ghost anchor always covers
                // the builder's cell (C++ `GameManager_GetBuilderUnitCursor`).
                let site = match app.build_site() {
                    Some(site) => format!(
                        "site=open@{},{} cursor={},{} valid={}",
                        site.anchor.x,
                        site.anchor.y,
                        site.cursor.x,
                        site.cursor.y,
                        u8::from(site.valid(app.state_ref()))
                    ),
                    None => "site=closed".to_string(),
                };
                // `Preferences`/`Setup` (`OptionsMenu`): the live test
                // (`max-brh-cicd/scripts/ui-test-prefs.ps1`) checks opening/closing.
                let preferences = if app.preferences_dialog().is_some()
                    || matches!(screen, Screen::Preferences(_))
                {
                    "prefs=open"
                } else {
                    "prefs=closed"
                };
                eprintln!(
                    "[in] screen={name} event={input:?} {alloc} {transfer} {build} {site} {preferences} \
                     effects={} sound={}",
                    u8::from(app.effects_enabled()),
                    u8::from(app.sound_enabled())
                );
            }

            if let Some(recorder) = recorder.as_mut() {
                recorder.record(
                    input,
                    RecordContext {
                        in_session: matches!(screen, Screen::InSession { .. }),
                        text_open: app.hud().chat.is_active(),
                    },
                );
            }

            if let InputEvent::MouseMove(x, y) = input {
                mouse = Some((x, y));
                // The position is needed both for drawing (menu button and M-HS turn-transition
                // window highlighting) and not only for input.
                ctx.mouse = Some(max_render::ui::Point::new(x, y));
            }

            // Inside a session input goes to the HUD (`handle_session_input`): selection,
            // orders 38/08, End Turn/Menu buttons. In the menu — the screen machine.
            // The press feedback is armed for both paths (the menu screens draw the
            // same `*D` pictures through `menu::draw_image_button`).
            match input {
                InputEvent::MouseClick(x, y) => app.note_button_press(x, y),
                InputEvent::MouseRelease(..) => app.release_button_press(),
                _ => {}
            }

            let screen_before = std::mem::discriminant(&screen);
            let in_session = matches!(screen, Screen::InSession { .. });
            let transition = if in_session {
                handle_session_input(&mut app, input)
            } else {
                screen.handle_input(&mut ctx, input)
            };

            // A menu click that changed the screen: the pressed picture belonged to
            // the old screen, do not light up a same-position button of the new one.
            if std::mem::discriminant(&screen) != screen_before {
                app.clear_button_press();
            }

            // Campaign stage 3a: `GameSetupMenu_Menu` (`menu.cpp:1972-1977`) writes a
            // `last_campaign` above the mission list back to the settings. The menu
            // layer only reports the clamp, the driver applies it.
            if let Some(last_campaign) = ctx.campaign_clamp.take() {
                app.apply_campaign_clamp(last_campaign);
            }

            // UI sounds (`Sfx`): voices — by `disable_voice`, effects — by
            // `disable_fx`/volume (C++ `SoundManager::PlaySfx`).
            for sfx in app.take_sounds() {
                let enabled = if sfx.is_voice() {
                    app.preferences().voice_enabled()
                } else {
                    app.sound_enabled()
                };

                if enabled {
                    let _ = ctx.audio.play_once(sfx.tag());
                }
            }

            // Combat sounds are **not** drained here: they belong to the frame of
            // the shot, not to an input event — see the frame-level
            // `App::present_combat_audio` call after the simulation steps below.

            match transition {
                Transition::None => {}
                Transition::Quit => {
                    finish_recording(&mut recorder)?;
                    return Ok(());
                }
                Transition::LeaveSession => {
                    // `menu.cpp:495-512`: after the game-over screen the mission's
                    // victory/defeat story is shown before the wrap-up returns to
                    // the main menu. Any other `LeaveSession` stays immediate.
                    let story = if matches!(screen, Screen::GameOver) {
                        let descriptor = active_mission.clone().or_else(|| {
                            let category = app.state_ref().mission_category();
                            let index =
                                usize::from(app.state_ref().mission_index()).checked_sub(1)?;
                            ctx.missions
                                .as_deref()?
                                .missions(category)
                                .get(index)
                                .cloned()
                        });
                        let victory = app
                            .local()
                            .and_then(|local| local.outcome.as_ref())
                            .is_some_and(|outcome| outcome.victory);
                        let kind = if victory {
                            StoryKind::Victory
                        } else {
                            StoryKind::Defeat
                        };
                        let data_dir = app
                            .config()
                            .max_res
                            .parent()
                            .map(std::path::Path::to_path_buf)
                            .unwrap_or_default();

                        StoryScreen::for_mission(
                            descriptor.as_ref(),
                            kind,
                            usize::from(app.state_ref().mission_index()),
                            app.assets().map(|assets| &assets.max_res),
                            &data_dir,
                        )
                    } else {
                        None
                    };

                    match story {
                        Some(mut story_screen) => {
                            ctx.status =
                                start_story_music(&mut music, &mut ctx.audio, &mut story_screen);
                            screen = Screen::MissionStory(Box::new(story_screen));
                            story_next = Some(StoryContinuation::LeaveSession);
                        }
                        None => {
                            story_next = None;
                            active_mission = None;
                            screen = Screen::MainMenu;
                            // `menu.cpp:518-545`: the wrap-up of a won campaign mission
                            // advances `last_campaign` in `App::preferences`; the menu reads
                            // the mirror, so it must follow the application.
                            app.leave_local_game();
                            ctx.preferences = app.preferences().clone();
                            ctx.status = "Left the session".to_string();
                        }
                    }
                }
                Transition::StoryFinished => {
                    finish_story_screen(
                        &mut screen,
                        &mut story_next,
                        &mut active_mission,
                        &mut app,
                        &mut ctx,
                        &mut music,
                    );
                }
                Transition::ApplyPreferences(preferences) => {
                    // `OptionsMenu` `Done`: the values are already saved in
                    // `preferences.ini`; we apply them to the application and the audio layer.
                    ctx.player_name = preferences.player_name.clone();
                    app.set_preferences(*preferences);
                    app.persist_preferences();
                    ctx.preferences = app.preferences().clone();
                    ctx.audio.set_enabled(app.sound_enabled());
                    ctx.audio.set_volume(app.preferences().sound_volume());
                }
                Transition::StartLocal(setup) => {
                    // M-SP: start of a local match (Options → Planet → Players).
                    match app.start_local_game(&setup) {
                        Ok(()) => {
                            ctx.status = String::new();
                            screen = Screen::InSession {
                                request: SessionRequest {
                                    game_id: "local".to_string(),
                                    password: None,
                                },
                            };
                        }
                        Err(error) => {
                            eprintln!("max-brh: local game: {error}");
                            ctx.status = format!("Local game: {error}");
                            // The start failed (e.g., no planet data for the
                            // phases) — we return to the menu instead of staying on the
                            // empty PreGame screen.
                            screen = Screen::MainMenu;
                        }
                    }
                }
                Transition::StartMission(request) => {
                    // Campaign stage 2: the `GameSetupMenu` chose a mission; the start
                    // goes through the stage-1 flow (`start_mission_from_registry`),
                    // never by mutating the match in the UI.
                    let started = match ctx.missions.as_deref() {
                        None => Err(
                            "mission registry is unavailable (PATCHES.RES is not loaded)"
                                .to_string(),
                        ),
                        Some(registry) => {
                            let setup = LocalSetup {
                                player_name: ctx.player_name.clone(),
                                ..LocalSetup::default()
                            };

                            app.start_mission_from_registry(
                                registry,
                                request.category,
                                request.index,
                                &setup,
                            )
                        }
                    };

                    match started {
                        Ok(start) => {
                            ctx.status = String::new();
                            // `GameSetupMenu_Menu` (`menu.cpp:2058`): the chosen
                            // mission is the persistent `game_file_number`.
                            app.persist_preferences();
                            println!(
                                "UI: миссия {} — {} ({:?}), файл {}",
                                start.tag,
                                start.title,
                                start.category,
                                start.path.display()
                            );
                            // Campaign stage 3b: keep the descriptor of the loaded
                            // mission (`MissionManager::GetMission`) for the
                            // victory/defeat story after the match.
                            active_mission = ctx.missions.as_deref().and_then(|registry| {
                                registry
                                    .missions(request.category)
                                    .get(request.index)
                                    .cloned()
                            });

                            let data_dir = app
                                .config()
                                .max_res
                                .parent()
                                .map(std::path::Path::to_path_buf)
                                .unwrap_or_default();
                            let story = active_mission.as_ref().and_then(|descriptor| {
                                StoryScreen::for_mission(
                                    Some(descriptor),
                                    StoryKind::Intro,
                                    usize::from(start.mission_index),
                                    app.assets().map(|assets| &assets.max_res),
                                    &data_dir,
                                )
                            });

                            match story {
                                Some(mut story_screen) => {
                                    // The match is already loaded: the story is a
                                    // presentation screen before the session, so a
                                    // missing story file can never block the start.
                                    ctx.status = start_story_music(
                                        &mut music,
                                        &mut ctx.audio,
                                        &mut story_screen,
                                    );
                                    screen = Screen::MissionStory(Box::new(story_screen));
                                    story_next = Some(StoryContinuation::EnterSession);
                                }
                                None => {
                                    story_next = None;
                                    screen = Screen::InSession {
                                        request: SessionRequest {
                                            game_id: "local".to_string(),
                                            password: None,
                                        },
                                    };
                                }
                            }
                        }
                        Err(error) => {
                            // The list stays on screen with the reason (condition 5):
                            // a missing file is visible, not a silent menu reset.
                            eprintln!("max-brh: mission: {error}");
                            ctx.status = format!("Mission: {error}");
                        }
                    }
                }
                Transition::StartSession(request) => {
                    pending = Some(request);
                    break;
                }
            }
        }

        // M-SP: local match tick, turn timer and AI turns.
        //
        // 1.4.2: `frame_dt` is presentation time (credits, camera, visible
        // movement); the simulation runs a whole number of fixed 60 Hz steps.
        // The same scenario therefore yields the same `snapshot_bytes` at 60 Hz,
        // 144 Hz or after a DPI change.
        let frame_seconds = last_frame.elapsed().as_secs_f64().clamp(0.0, 0.25);
        last_frame = std::time::Instant::now();
        let frame_dt = frame_seconds as f32;
        let simulation_steps = step_clock.plan(frame_seconds);

        // Menu/story screen animation (`Credits`, `Introduction`, campaign story
        // videos): `dt` is the real frame time, `dt = 0` keeps a freeze-frame.
        if matches!(screen, Screen::MissionStory(_)) {
            if screen.advance(frame_dt) {
                finish_story_screen(
                    &mut screen,
                    &mut story_next,
                    &mut active_mission,
                    &mut app,
                    &mut ctx,
                    &mut music,
                );
            }
        } else if screen.advance(frame_dt) {
            let was_intro = matches!(screen, Screen::Introduction(_));

            screen = Screen::MainMenu;
            // The original main menu has no status line; a technical line must not
            // remain after the movie (and the GNW font would draw `?` for Cyrillic).
            if was_intro {
                ctx.status.clear();
            } else {
                ctx.status = "Credits ended".to_string();
            }
        }

        // C3: the audio track of the `Introduction` movie and of the story video.
        // The chunks were decoded together with the frames; queue them in order (the
        // oracle's `sndAdd`), gated by `disable_movie`/`movie_level`.
        let movie_chunks = match &mut screen {
            Screen::Introduction(screen) => Some(screen.take_audio()),
            Screen::MissionStory(story) => Some(story.take_audio()),
            _ => {
                ctx.audio.stop_movie_audio();
                None
            }
        };

        if let Some(chunks) = movie_chunks {
            let enabled = app.preferences().movie_enabled() && ctx.audio.is_audible();
            let gain = app.preferences().movie_volume();

            for chunk in chunks {
                if enabled {
                    ctx.audio
                        .play_pcm(&chunk.samples, chunk.channels, chunk.sample_rate, gain);
                }
            }
        }

        if matches!(screen, Screen::InSession { .. }) && app.local_active() {
            // Fixed-step simulation: a slow or fast render frame only decides
            // how many 60 Hz steps run, never how much time one tick covers.
            let mut update = max_app::local::LocalUpdate::default();

            for _ in 0..simulation_steps {
                let step = app.local_update(max_app::presentation::SIMULATION_DT);

                update.ticked |= step.ticked;
                update.turn_closed |= step.turn_closed;
                update.ai_started |= step.ai_started;
                update.finished |= step.finished;
            }

            if std::env::var_os("MAX_DEBUG_INPUT").is_some() {
                let tick = app.ticks();
                if tick % 10 == 0 || tick <= 3 {
                    let player = app.state_ref().player_team();
                    let mut units: Vec<String> = app
                        .state_ref()
                        .unit_ids_in_order()
                        .iter()
                        .filter_map(|id| app.state_ref().unit(*id))
                        .filter(|u| u.team == player)
                        .take(12)
                        .map(|u| {
                            // `vX.Y` — the visual position of the sprite: with `dt > 0`
                            // it catches up with `grid` along the actual route.
                            let (vx, vy) = app
                                .visual_position(u.id)
                                .unwrap_or((f32::from(u.grid.x), f32::from(u.grid.y)));
                            format!(
                                "{}@({},{})v{:.1},{:.1}",
                                u.unit_type as u8, u.grid.x, u.grid.y, vx, vy
                            )
                        })
                        .collect();
                    units.sort();
                    let phase = app
                        .local()
                        .map(|local| local.phase_caption())
                        .unwrap_or_default();
                    let selected = app
                        .hud()
                        .selected
                        .map(|id| format!("sel={}", id.0))
                        .unwrap_or_else(|| "sel=-".to_string());
                    let preview = app
                        .hud()
                        .preview
                        .as_ref()
                        .map(|preview| preview.len())
                        .unwrap_or(0);
                    eprintln!(
                        "[tick {tick}] turn={} phase={phase} {selected} preview={preview} units={}",
                        app.state_ref().turn_counter(),
                        units.join(" ")
                    );
                }
            }

            if let Some(local) = app.local() {
                ctx.status = local.status_line(app.state_ref().turn_counter());
            }

            if update.finished || app.local().is_some_and(|local| local.finished()) {
                screen = Screen::GameOver;
                ctx.status = "Game over — Enter/Esc returns to the menu".to_string();
            }
        }

        // Planet minimaps for the setup screen (`.WRL` is loaded once).
        // The help window can be open over the setup screen.
        let setup_screen = match &mut screen {
            Screen::Help { screen, .. } => screen.as_mut(),
            other => other,
        };

        if let Screen::LocalSetup(form) = setup_screen {
            let palette = app
                .assets()
                .and_then(|assets| assets.menu.as_ref())
                .map(|menu| menu.palette);
            form.ensure_minimaps(palette.as_ref());
        }

        // Mouse cursor: menu/panels — HANDPTR, map — MAP_PTR/orders/units
        // (`Cursor_SetCursor` + `GameManager_GetWindowCursor`). Without SDL cursors
        // (or without loaded resources) the system cursor remains.
        update_window_cursor(&mut window, &app, &screen, mouse);

        // C++ `settings["disable_fx"]`/`fx_sound_level`: the flag reaches
        // the audio layer in the same frame in which it was changed in Preferences.
        ctx.audio.set_enabled(app.sound_enabled());
        ctx.audio.set_volume(app.preferences().sound_volume());

        // Combat sounds of this frame (`UnitInfo::PrepareFire` → `SFX_TYPE_FIRE`,
        // `UnitsManager_StartExplosion` → `SFX_TYPE_HIT`/`SFX_TYPE_EXPLOAD`; the
        // tag and volume come from `SC_UNITS`, the switch from `disable_fx`).
        //
        // The drain is frame-level, right after the fixed-step ticks and before the
        // frame is drawn, because that is when C++ plays the shot: `PlaySfx` runs
        // synchronously inside `PrepareFire` (`unitinfo.cpp:4725`) just before the
        // firing frame is set, and `process_bk`/`ProcessJobs` started the job on the
        // same frame (`sound_manager.cpp:646`). Draining the queue from the
        // input-event loop above delayed every shot until the next mouse/key event
        // and turned everything queued meanwhile into one late volley.
        app.present_combat_audio(&mut ctx.audio);

        // D2: the background music follows the screen (`MAIN_MSC` in the menu, the planet
        // shuffle track in a match) and the facade event stream (win/lose). The director
        // only sees `GameEvent`s — it can neither read nor mutate the simulation state.
        let scene = match &screen {
            Screen::InSession { .. } | Screen::GameOver => MusicScene::InGame,
            // `Movie_PlayIntro` runs before `PlayMusic(MAIN_MSC)` (`main.cpp:39-43`):
            // no scene track over the movie audio.
            Screen::Introduction(_) => MusicScene::Intro,
            // The victory/defeat story belongs to the finished match; the intro one
            // still runs in the menu (the oracle keeps the menu track there).
            Screen::MissionStory(story) if story.kind() != StoryKind::Intro => MusicScene::InGame,
            _ => MusicScene::Menu,
        };
        music.set_enabled(app.preferences().music_enabled(), &mut ctx.audio);
        music.set_volume(app.preferences().music_volume(), &mut ctx.audio);

        let events = app.take_music_events();
        let planet = app.config().planet;
        let player_team = app.state_ref().player_team();
        music.sync(scene, &events, player_team, planet, &mut ctx.audio);
        music.advance_if_finished(&mut ctx.audio, planet);

        if music.messages().len() > reported_music_errors {
            for message in &music.messages()[reported_music_errors..] {
                eprintln!("UI: {message}");
            }

            reported_music_errors = music.messages().len();
        }

        if let Some(request) = pending.take() {
            if runtime.is_none() {
                runtime = Some(build_ui_runtime()?);
            }
            let runtime = runtime.as_mut().expect("runtime создан выше");

            // "Connecting" frame: while Horizon/the lobby run, the window is not empty.
            ctx.status = format!("Connecting to {}…", request.game_id);
            screen.render(&mut buffer, ctx.view(Some(&app)));
            let _ = window.present(&buffer);

            let status = match run_ui_session(
                runtime,
                &mut app,
                &mut window,
                &screen,
                &request,
                cli.turns.unwrap_or(0),
                reconnect,
                join_retry,
                Duration::from_secs(u64::from(cli.turn_wait_secs.unwrap_or(0))),
                recorder.as_mut(),
            ) {
                Ok(report) => {
                    print_network(&report);
                    format!(
                        "Session {} finished: turns={} ticks={}",
                        report.game_id, report.turns_finished, report.ticks
                    )
                }
                Err(error) => {
                    eprintln!("max-brh: UI session: {error:#}");
                    format!("Session failed: {error:#}")
                }
            };

            screen = Screen::MainMenu;
            ctx.status = status;
            continue;
        }

        screen.render(&mut buffer, ctx.view(Some(&app)));
        window
            .present(&buffer)
            .map_err(|error| anyhow!("{error}"))?;

        // 1.4.2: 60 Hz frame budget evidence. The pacer sleeps one period after
        // the frame; the stats count how many frames exceeded 16.67 ms.
        frame_stats.record(frame_started.elapsed());

        // `--fps` is the interactive 60 Hz probe: a line per second is useful for
        // a manual run; the profile environment keeps the 10-second cadence.
        let stats_interval = if cli.fps_stats { 60 } else { 600 };

        if (cli.fps_stats || max_app::profile::enabled())
            && frame_stats.frames - logged_frames >= stats_interval
        {
            logged_frames = frame_stats.frames;
            eprintln!(
                "UI: {} dropped_steps={} steps_now={}",
                frame_stats.summary(),
                step_clock.dropped_steps(),
                simulation_steps
            );
        }

        pacer.wait();
    }

    finish_recording(&mut recorder)?;

    Ok(())
}

/// Appends to the `.qa` file on exit (trailing `type`/`wait`, final
/// comment) and prints the resulting path. Without `--record` — no-op.
#[cfg(feature = "sdl")]
fn finish_recording(recorder: &mut Option<Recorder>) -> Result<()> {
    let Some(recorder) = recorder.as_mut() else {
        return Ok(());
    };

    if let Some(path) = recorder.finish().context("финализация записи")? {
        println!("RECORD: сохранено {}", path.display());
    }

    Ok(())
}

/// GUI (launch without flags or `--ui`) without the `sdl` feature (a build with
/// `--no-default-features`): a clear error instead of "no window".
#[cfg(not(feature = "sdl"))]
fn run_ui(_cli: &Cli) -> Result<()> {
    bail!(
        "GUI-режим (запуск без флагов или --ui) требует сборки с feature sdl \
         (входит в default-features): cargo run -p max_app; \
         консольная сборка клиента — с --no-default-features"
    )
}

/// Sets the window cursor by context (`Cursor_SetCursor`).
///
/// Menu/form/game list — `HANDPTR`; in a match — [`max_app::cursor::session_cursor`]
/// (map/panel/unit/order). Cursor resources are loaded from `MAX.RES`
/// (`CursorSet`), the palette — the window system palette (`FRAMEPIC`).
#[cfg(feature = "sdl")]
fn update_window_cursor(
    window: &mut max_render::sdl_window::SdlWindow,
    app: &App,
    screen: &Screen,
    mouse: Option<(i32, i32)>,
) {
    let Some(assets) = app.assets() else {
        return;
    };
    let Some(cursors) = assets.cursors.as_ref() else {
        return;
    };

    let (cursor, frame) = if app.help_screen().is_some() {
        // `Cursor_SetCursor(CURSOR_HELP)` in `HelpMenu_Menu`.
        (max_app::CursorId::Help, 0)
    } else if app.research_menu().is_some() {
        // `ResearchMenu::ResearchMenu`: `Cursor_SetCursor(CURSOR_HAND)`.
        (max_app::CursorId::Hand, 0)
    } else if app.preferences_dialog().is_some() {
        // `OptionsMenu::OptionsMenu`: `Cursor_SetCursor(CURSOR_HAND)`.
        (max_app::CursorId::Hand, 0)
    } else {
        match screen {
            Screen::InSession { .. } => max_app::cursor::session_cursor(app, mouse),
            // `Cursor_SetCursor(CURSOR_HELP)` in `HelpMenu_Menu`.
            Screen::Help { .. } => (max_app::CursorId::Help, 0),
            // `GameManager_GetWindowCursor` on the landing screen: UNIT_GO over
            // a valid cell, UNIT_NO_GO over an invalid one, HANDPTR outside the map.
            Screen::PreGame(flow) => match &flow.stage {
                max_app::PreGameStage::Landing(landing) => match mouse {
                    Some((x, y)) => {
                        match max_app::landing::cursor_for_screen_point(landing, x, y) {
                            Some(cursor) => (cursor, 0),
                            None => (max_app::CursorId::Hand, 0),
                        }
                    }
                    None => (max_app::CursorId::Hand, 0),
                },
                _ => (max_app::CursorId::Hand, 0),
            },
            _ => (max_app::CursorId::Hand, 0),
        }
    };
    let palette = max_app::render::window_palette(assets);
    let Some(image) = max_app::cursor::cursor_image(cursors, &palette, cursor, frame) else {
        return;
    };
    let key = cursor.index() as u32 | ((frame as u32) << 8);

    if let Err(error) = window.set_cursor_rgba(
        key,
        &image.rgba,
        image.width,
        image.height,
        image.hot_x,
        image.hot_y,
    ) {
        tracing::debug!("SDL-курсор: {error}");
    }
}

#[cfg(feature = "sdl")]
fn run_ui_session(
    runtime: &tokio::runtime::Runtime,
    app: &mut App,
    window: &mut max_render::SdlWindow,
    screen: &Screen,
    request: &SessionRequest,
    max_turns: u32,
    reconnect: ReconnectPolicy,
    join_retry: JoinRetryPolicy,
    turn_wait: Duration,
    recorder: Option<&mut Recorder>,
) -> Result<NetworkReport> {
    let options = NetworkOptions {
        game_id: request.game_id.clone(),
        password: request.password.clone(),
        max_turns,
        reconnect,
        join_retry,
        turn_wait,
        ..NetworkOptions::default()
    };
    let (width, height) = (app.config().screen_width, app.config().screen_height);
    let mut sink = UiSessionSink {
        window,
        screen: screen.clone(),
        buffer: max_render::buffer::RgbaBuffer::new(width, height),
        status: String::new(),
        game_id: request.game_id.clone(),
        recorder,
    };

    runtime.block_on(run_network_with_sink(app, &options, Some(&mut sink)))
}

/// Session frames: input (mouse/keys) → HUD, map + HUD → window.
///
/// The two hooks are the phases of the shared session step (SDL/UI Stage C1):
/// [`max_app::net::FrameSink::poll_input`] and
/// [`max_app::net::FrameSink::render`], run by `net::run_game_loop` around the one
/// tick. Input is handled by [`handle_session_input`]: a click on an own unit —
/// selection (a repeated click opens its menu), LMB on a cell — a move order
/// (packet 38), a click on an enemy — attack (08), RMB — select/inspect or cancel
/// the order modes, the buttons End Turn/Menu. The session channel decides what
/// happens to the orders (SDL/UI Stage C2): the network loop collects them itself
/// (`net::flush_hud_orders`) — no `online` flag is passed.
#[cfg(feature = "sdl")]
struct UiSessionSink<'a> {
    window: &'a mut max_render::SdlWindow,
    screen: Screen,
    buffer: max_render::buffer::RgbaBuffer,
    status: String,
    game_id: String,
    /// QA layer 2.5: the same event stream that goes to the HUD.
    recorder: Option<&'a mut Recorder>,
}

#[cfg(feature = "sdl")]
impl max_app::net::FrameSink for UiSessionSink<'_> {
    fn poll_input(&mut self, app: &mut App<NetworkDispatcher>) -> bool {
        app.update_visual_units(1.0 / 60.0);

        // 1. This frame's input → HUD (selection, orders, buttons).
        if let Some(recorder) = self.recorder.as_deref_mut() {
            recorder.begin_frame();
        }

        let (running, events) = self.window.poll_input();
        let mut leave = false;

        for event in events {
            let input = input_event(event);

            if let Some(recorder) = self.recorder.as_deref_mut() {
                recorder.record(
                    input,
                    RecordContext {
                        in_session: true,
                        text_open: app.hud().chat.is_active(),
                    },
                );
            }

            match handle_session_input(app, input) {
                Transition::None => {}
                Transition::LeaveSession | Transition::Quit => leave = true,
                Transition::ApplyPreferences(preferences) => {
                    app.set_preferences(*preferences);
                    app.persist_preferences();
                }
                Transition::StartSession(_)
                | Transition::StartLocal(_)
                | Transition::StartMission(_)
                | Transition::StoryFinished => {}
            }
        }

        if !running || leave {
            return false;
        }

        // Preferences active in any match (including network):
        // edge scrolling (`click_scroll`/`quick_scroll`) and the camera following
        // the selection (`follow_unit`). Auto-selection (`auto_select`) is enabled
        // in `App::tick`, which the session step runs.
        app.update_scroll(1.0 / 60.0);
        app.follow_selected_unit();

        true
    }

    fn render(&mut self, app: &mut App<NetworkDispatcher>) -> bool {
        // 2. Frame: map + HUD over it.
        self.status = format!(
            "IN SESSION · {} · tick={} turn={} · LMB select/move · RMB select/cancel · Esc cancel",
            self.game_id,
            app.ticks(),
            app.state_ref().turn_counter()
        );

        let view = ScreenView {
            width: self.buffer.width(),
            height: self.buffer.height(),
            status: &self.status,
            mouse: None,
            focus: 0,
            app: Some(app),
            missions: None,
        };
        self.screen.render(&mut self.buffer, view);

        // 1.4.2 alt-tab gate: while the window is minimized/occluded the frame
        // is not uploaded, but the network loop keeps ticking — barrier
        // timeouts and turn timers are not affected by focus.
        if self.window.should_present() {
            if let Err(error) = self.window.present(&self.buffer) {
                eprintln!("SDL: {error}");
                return false;
            }
        }

        true
    }

    /// The "session is warming up… attempt N" line over the window: while
    /// `SessionHandle::start` is repeated, there are no match frames yet.
    fn join_status(&mut self, message: &str) -> bool {
        self.status = message.to_string();
        // No match yet — the menu application type (default dispatcher).
        let view: ScreenView<'_> = ScreenView {
            width: self.buffer.width(),
            height: self.buffer.height(),
            status: &self.status,
            mouse: None,
            focus: 0,
            app: None,
            missions: None,
        };
        self.screen.render(&mut self.buffer, view);

        if self.window.should_present() {
            if let Err(error) = self.window.present(&self.buffer) {
                eprintln!("SDL: {error}");
                return false;
            }
        }

        true
    }
}

// ---------------------------------------------------------------------------
// Value parsing
// ---------------------------------------------------------------------------

/// `--scale`: `None`/`0` — fit to the desktop, otherwise 1..=8.
#[cfg(feature = "sdl")]
fn normalized_window_scale(scale: Option<u32>) -> Option<u32> {
    match scale {
        None | Some(0) => None,
        Some(value) => Some(value.clamp(1, max_render::frame_scale::MAX_SCALE)),
    }
}

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

/// `--connect <host[:port]>` without `--join`: the direct P2P host endpoint.
///
/// The fallback port is the host's ENet port ([`DEFAULT_HOST_PORT`] = 31554),
/// not the Horizon gRPC port: no Horizon RPC is involved in the host mode.
fn parse_direct_endpoint(value: &str) -> Result<(String, u16)> {
    match value.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => {
            let port: u16 = port
                .parse()
                .with_context(|| format!("некорректный порт в {value}"))?;
            Ok((host.to_string(), port))
        }
        _ => Ok((value.to_string(), DEFAULT_HOST_PORT)),
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

/// Repository root, if `cargo run` is launched from `max-brh/` or the root.
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

fn print_help() {
    println!(
        "\
max-brh — клиент M.A.X. (core + net + assets + render)

ИСПОЛЬЗОВАНИЕ:
    max-brh.exe [ОПЦИИ]                     # по умолчанию — GUI (окно SDL3)
    max-brh.exe --demo <save> [--turns N] [ОПЦИИ]
    max-brh.exe --new-game [ОПЦИИ]
    max-brh.exe --host [--listen <ip>] [--host-port <port>] [--min-players N] [--host-policy pause|eliminate] [--turns N] [ОПЦИИ]
    max-brh.exe --connect <host:port>                   # прямой гость P2P-хоста (режим хоста)
    max-brh.exe --connect <host[:port]> --join <game_id> [--sdl] [ОПЦИИ]

РЕЖИМЫ:
    (без флагов)         интерактивное меню: создать/выбрать игру и играть
                         (GUI по умолчанию, feature sdl в default-features;
                         то же, что --ui)
    --ui                 совместимый alias для запуска по умолчанию (GUI)
    --demo <save>        V70-сейв → N тиков → digest (headless)
    --new-game           локальная партия (setup::generate_map) + кадр
    --host               сервер P2P-режима: ENet + лобби 28…34 и свой
                         игровой цикл за команду хоста (headless)
    --listen <ip>        адрес ENet-сервера хоста (по умолчанию 0.0.0.0)
    --host-port <port>   UDP-порт ENet-сервера хоста (31554)
    --min-players N      READY-порог лобби, включая хоста (2; 1..=4)
    --host-policy P      политика сессии: pause (по умолчанию) | eliminate
    --connect <host>     Horizon gRPC (порт по умолчанию {}) + ENet-сессия;
                         без --join — прямой адрес ENet-сервера хоста
    --join <game_id>     игра для --connect (режим Horizon)
    --sdl                окно SDL3 для --connect --join (feature sdl — в default)
    --record <file.qa>   записать ручную игру в QA-сценарий (GUI; --ui не нужен;
                         существующий файл не перезаписывается — суффикс -1;
                         без SDL запись доступна как API max_app::qa::Recorder)

ИГРА:
    --turns N            тиков в --demo (5) / лимит ходов в --connect и --host
                         (0 — без лимита)
    --reconnect-attempts N   попыток переподключения при разрыве ENet (3; 0 — выкл.)
    --reconnect-timeout S    таймаут одной попытки переподключения в секундах (10)
    --join-retry-attempts N  попыток первого подключения после Create game (6;
                             0 — одна попытка без ретраев)
    --join-retry-timeout S   таймаут одной попытки подключения в секундах (4);
                             backoff 1,2,3,5,8,11 с, окно ретраев ~30 с
    --turn-wait N        окно ввода в фазе PLAY сессии, секунды (0 — сразу)
    --scale N            масштаб окна: кадр 640x480 умножается на N (0/нет —
                         подобрать под рабочий стол, nearest-neighbour)
    --fullscreen         окно в безрамочном полноэкранном режиме на весь монитор
                         (Alt+Enter переключает в игре; рендер тот же кадр)
    --fps                печатать статистику 60 Гц (`PRESENT: …`) в stderr
                          раз в 600 кадров (бюджет 16.67 мс)
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
                                           (необязательно: без флага клиент сам
                                           пробует полный хэш, затем только
                                           MAX.RES — сервер без PATCHES.RES)
    --width/--height N   размер **кадра** (по умолчанию 640x480 — окно
                         оригинала); окно = кадр × --scale

ПРИМЕРЫ:
    max-brh.exe                                     # GUI: меню + партия
    max-brh.exe --demo max-original\\SAVE1.CAM --turns 5
    max-brh.exe --new-game --world 12 --seed 42 --out frame.ppm
    max-brh.exe --connect 127.0.0.1:8082 --join demo --sdl
    cargo run -p max_app                             # из max-brh/: GUI (default)
    cargo run -p max_app --features sound            # GUI + звук
    cargo run -p max_app --no-default-features -- --demo ...   # headless

ОКРУЖЕНИЕ:
    MAX_LOG=debug|trace           уровень логов (RUST_LOG тоже читается;
                                  без env-filter — простой глобальный уровень)
",
        DEFAULT_GRPC_PORT
    );
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
