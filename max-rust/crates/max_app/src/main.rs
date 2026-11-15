//! `max-rust` — CLI клиента M.A.X. (крейт/библиотека — `max_app`).
//!
//! Режимы:
//!
//! * без флагов (или `--ui`) — интерактивное меню (создать/выбрать игру) и
//!   сессия в окне SDL3: это режим по умолчанию, feature `sdl` входит в
//!   default (`sdl-from-source`), т.е. `cargo run -p max_app` сразу открывает
//!   игру;
//! * `--demo <save> [--turns N]` — headless: V70-сейв → N тиков → digest;
//! * `--new-game` — локальная партия на карте планеты + кадр в PPM (`--out`);
//! * `--connect <horizon> --join <game> [--sdl]` — Horizon + ENet-сессия;
//!   без `--sdl` цикл headless, с `--sdl` — окно SDL3 (feature `sdl`).
//!
//! Вся игровая логика — в библиотеке (`max_app`), здесь только разбор
//! аргументов и SDL-обвязка. Примеры — в `README.md` крейта.

// Windows: GUI-сборки (`--features sdl*`) — подсистема `windows`, поэтому
// двойной клик по `max-rust.exe` не открывает консоль. Headless-сборка без
// `sdl` остаётся console: `--demo`/`--new-game`/`--connect` печатают в stdout
// терминала и CI. `sdl-vcpkg` и `sdl-from-source` включают `sdl` (см.
// Cargo.toml), поэтому одного cfg достаточно для всех оконных фич.
#![cfg_attr(feature = "sdl", windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use max_app::assets::AssetsReport;
#[cfg(feature = "sdl")]
use max_app::audio::AudioPlayer;
#[cfg(feature = "sdl")]
use max_app::net::run_network_with_sink;
#[cfg(feature = "sdl")]
use max_app::qa::{RecordContext, Recorder};
use max_app::net::{run_network, JoinRetryPolicy, NetworkOptions, NetworkReport, ReconnectPolicy};
#[cfg(feature = "sdl")]
use max_app::ui::{
    handle_session_input, HorizonMenuClient, InputEvent, Key, Screen, ScreenView, SessionRequest,
    Transition, UiContext,
};
use max_app::{write_ppm, App, AppConfig, AppMode, NewGameConfig, NewGameReport};
use max_core::enums::{Planet, TeamClan};
use max_core::setup::ResourceLevel;
#[cfg(feature = "sdl")]
use max_render::sdl_window::{WindowInput, WindowKey};

const DEFAULT_SAVE: &str = "max-original/SAVE1.CAM";
const DEFAULT_GRPC_PORT: u16 = 8082;

/// Что запускать.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Demo,
    NewGame,
    Connect,
    Ui,
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
    /// M6.1-заготовка: попытки переподключения при разрыве (0 — выключить).
    reconnect_attempts: Option<u32>,
    /// M6.1-заготовка: таймаут одной попытки переподключения в секундах.
    reconnect_timeout_secs: Option<u32>,
    /// Сколько попыток первого подключения (Create game → Join → start).
    join_retry_attempts: Option<u32>,
    /// Таймаут одной попытки первого подключения в секундах.
    join_retry_timeout_secs: Option<u32>,
    /// Окно ввода в фазе PLAY сессии, секунды (`0`/нет — headless: ход
    /// закрывается сразу; см. `max_app::net::NetworkOptions::turn_wait`).
    turn_wait_secs: Option<u32>,
    /// Масштаб окна UI: `0`/нет — подобрать под рабочий стол (пункт F).
    window_scale: Option<u32>,
    /// Куда писать `.qa`-сценарий ручной игры (`--ui --record`).
    record: Option<PathBuf>,
}

impl Cli {
    fn parse() -> Result<Self> {
        let cli = Self::parse_from(std::env::args().skip(1))?;
        cli.ensure_gui_available()?;
        Ok(cli)
    }

    /// Разбор аргументов без проверки наличия SDL-фичи (её проверяет
    /// [`Cli::parse`]): так CLI-тесты не зависят от feature-набора сборки.
    fn parse_from(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut cli = Self {
            mode: Mode::Ui,
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
            reconnect_attempts: None,
            reconnect_timeout_secs: None,
            join_retry_attempts: None,
            join_retry_timeout_secs: None,
            turn_wait_secs: None,
            window_scale: None,
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
                "--ui" => {
                    cli.mode = Mode::Ui;
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
                "--record" => {
                    cli.record =
                        Some(PathBuf::from(args.next().context("--record требует путь к .qa")?));
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

        if cli.sdl && cli.mode != Mode::Connect {
            bail!("--sdl имеет смысл только с --connect <horizon> --join <game>");
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
        if cli.mode == Mode::Connect && cli.game_id.is_none() {
            bail!("--connect требует --join <game_id>");
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
            Mode::Ui => AppMode::Ui,
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

    /// GUI (режим по умолчанию, он же `--ui`) требует feature `sdl` — она в
    /// default (`sdl-from-source`), поэтому сюда попадаем только у сборки с
    /// `--no-default-features`. Headless-режимы (`--demo`/`--new-game`/
    /// `--connect` без `--sdl`) SDL не требуют.
    #[cfg(not(feature = "sdl"))]
    fn ensure_gui_available(&self) -> Result<()> {
        if self.mode == Mode::Ui {
            bail!(
                "GUI-режим (запуск без флагов или --ui) требует сборки с feature sdl \
                 (в default-features), а этот бинарь собран с --no-default-features: \
                 cargo run -p max_app (без --no-default-features)\n\
                 headless-режимы: --demo <save>, --new-game, --connect <host> --join <game>"
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

fn main() -> ExitCode {
    tracing_subscriber::fmt().init();

    match Cli::parse().and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("max-rust: {error:#}");
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
            Mode::Ui => unreachable!("--ui разобран выше"),
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

    // Как в оригинале: к началу партии выбран первый боеспособный юнит игрока
    // со спрайтом — тогда кадр `--new-game` сразу показывает портрет,
    // статистику и описание, а не пустые панели.
    let selected = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state().unit(id))
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
    let mut reconnect = ReconnectPolicy::default();
    if let Some(attempts) = cli.reconnect_attempts {
        reconnect.attempts = attempts;
    }
    if let Some(seconds) = cli.reconnect_timeout_secs {
        reconnect.timeout = Duration::from_secs(u64::from(seconds));
    }
    let options = NetworkOptions {
        game_id: cli.game_id.clone().expect("validated by CLI"),
        password: cli.password.clone(),
        max_turns: cli.turns.unwrap_or(0),
        reconnect,
        join_retry: join_retry_policy(cli),
        turn_wait: Duration::from_secs(u64::from(cli.turn_wait_secs.unwrap_or(0))),
        ..NetworkOptions::default()
    };

    let report = if cli.sdl {
        connect_interactive(&mut app, &options, &cli.config, cli.window_scale).await?
    } else {
        run_network(&mut app, &options).await?
    };

    print_network(&report);

    Ok(())
}

/// Политика ретрая первого подключения из CLI (`--join-retry-*`).
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
        "network: game={} handshake={} lobby={} team={} ticks={} commands={} turns={} barrier_timeouts={} reconnects={} reconnect_attempts={} reconnect_pending_state={} join_attempts={} data_hash={}({})",
        report.game_id,
        report.handshake,
        report.lobby_complete,
        report.local_team.0,
        report.ticks,
        report.commands_applied,
        report.turns_finished,
        report.barrier_timeouts,
        report.reconnects,
        report.reconnect_attempts,
        report.reconnect_pending_state,
        report.join_attempts,
        &report.data_hash[..report.data_hash.len().min(8)],
        report.data_hash.len()
    );
    println!("digest: {}", report.digest);
}

/// Один SDL-ввод окна → [`InputEvent`] UI.
///
/// Левая кнопка — выбор/атака, правая — приказ движения/отмена (M4.3).
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
        WindowInput::Key(WindowKey::Char(character)) => InputEvent::Key(Key::Char(character)),
        WindowInput::Key(WindowKey::Other) => InputEvent::Key(Key::Other),
        WindowInput::MouseMove { x, y } => InputEvent::MouseMove(x, y),
        WindowInput::MouseClick { x, y } => InputEvent::MouseClick(x, y),
        WindowInput::MouseRightClick { x, y } => InputEvent::RightClick(x, y),
    }
}

/// Окно SDL3: кадр библиотеки → текстура → экран.
#[cfg(feature = "sdl")]
struct WindowSink {
    window: max_render::SdlWindow,
}

#[cfg(feature = "sdl")]
impl max_app::net::FrameSink for WindowSink {
    fn frame(&mut self, app: &mut App) -> bool {
        app.update_visual_units(1.0 / 60.0);
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
    scale: Option<u32>,
) -> Result<NetworkReport> {
    #[cfg(feature = "sdl")]
    {
        let window = max_render::SdlWindow::new_scaled(
            "M.A.X. — max_app (R2.4)",
            config.screen_width,
            config.screen_height,
            normalized_window_scale(scale),
        )
        .map_err(|error| anyhow!("{error}"))?;
        let mut sink = WindowSink { window };

        println!(
            "SDL: кадр {}x{}, окно {}x{} (масштаб {}); Esc/закрытие — выход",
            config.screen_width,
            config.screen_height,
            sink.window.size().0,
            sink.window.size().1,
            sink.window.frame_scale().scale()
        );

        run_network_with_sink(app, options, Some(&mut sink)).await
    }

    #[cfg(not(feature = "sdl"))]
    {
        let _ = (app, options, config, scale);
        bail!(
            "--sdl требует сборки с feature sdl (входит в default-features): \
             cargo run -p max_app -- --connect ... --join ... --sdl"
        )
    }
}

// ---------------------------------------------------------------------------
// --ui: интерактивное меню в окне SDL3
// ---------------------------------------------------------------------------

#[cfg(feature = "sdl")]
fn build_ui_runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("tokio runtime (UI)")
}

/// SDL-цикл UI: события окна → [`InputEvent`] → [`Screen`] → кадр.
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

    let mut window = SdlWindow::new_scaled(
        "M.A.X. — Rust client (M4.2 HUD)",
        config.screen_width,
        config.screen_height,
        normalized_window_scale(cli.window_scale),
    )
    .map_err(|error| anyhow!("{error}"))?;
    let mut ctx = UiContext::new(
        Box::new(HorizonMenuClient::new(
            &config.horizon_host,
            config.horizon_port,
        )),
        config.screen_width,
        config.screen_height,
    );
    // M6.2: звук меню. `with_default_backend` за feature `sound` поднимает
    // rodio/cpal; без фичи (или без устройства) — тихий no-op. Ссылки на
    // файлы берутся из MAX.RES, сами файлы — рядом с MAX.RES.
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
    let mut screen = Screen::MainMenu;
    let mut buffer = RgbaBuffer::new(config.screen_width, config.screen_height);
    let mut mouse: Option<(i32, i32)> = None;
    let mut runtime: Option<tokio::runtime::Runtime> = None;
    let mut pending: Option<SessionRequest> = None;
    // M-SP: имя игрока и каталог карт для настройки и мини-карт планет.
    ctx.player_name = config.player_name.clone();
    ctx.world_dir = config
        .world_dir
        .clone()
        .or_else(|| config.max_res.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_default();
    let mut last_frame = std::time::Instant::now();
    // M6.1-заготовка: политика реконнекта, общая для `--connect` и `--ui`.
    let mut reconnect = ReconnectPolicy::default();
    if let Some(attempts) = cli.reconnect_attempts {
        reconnect.attempts = attempts;
    }
    if let Some(seconds) = cli.reconnect_timeout_secs {
        reconnect.timeout = Duration::from_secs(u64::from(seconds));
    }
    // Create game → InSession: сессия headless греется 10–15 с, поэтому первый
    // `SessionHandle::start` повторяется с backoff, а не падает в меню.
    let join_retry = join_retry_policy(cli);

    println!(
        "UI: кадр {}x{}, окно {}x{} (масштаб {}); меню — мышь/стрелки, Enter; \
         в игре — LMB выбор, RMB движение, клик по врагу — атака, End Turn; Esc — выход",
        config.screen_width,
        config.screen_height,
        window.size().0,
        window.size().1,
        window.frame_scale().scale()
    );

    // Слой 2.5 QA: запись ручной игры в `.qa`-сценарий. Пишем тот же
    // `InputEvent` (уже в пикселях кадра 640×480), который получают экраны;
    // старт — сразу, с главного меню, финал — при выходе.
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
        if let Some(recorder) = recorder.as_mut() {
            recorder.begin_frame();
        }

        let (running, events) = window.poll_input();

        if !running {
            break;
        }

        for event in events {
            let input = input_event(event);

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
                // Позиция нужна и отрисовке (подсветка кнопок меню и окна
                // перехода хода M-HS), а не только вводу.
                ctx.mouse = Some(max_render::ui::Point::new(x, y));
            }

            // Внутри сессии ввод идёт в HUD (`handle_session_input`): выделение,
            // приказы 38/08, кнопки End Turn/Menu. В меню — машина экранов.
            let in_session = matches!(screen, Screen::InSession { .. });
            let transition = if in_session {
                handle_session_input(&mut app, input, false)
            } else {
                screen.handle_input(&mut ctx, input)
            };

            match transition {
                Transition::None => {}
                Transition::Quit => {
                    finish_recording(&mut recorder)?;
                    return Ok(());
                }
                Transition::LeaveSession => {
                    screen = Screen::MainMenu;
                    app.leave_local_game();
                    ctx.status = "Left the session".to_string();
                }
                Transition::StartLocal(setup) => {
                    // M-SP: старт локальной партии (Options → Planet → Players).
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
                            eprintln!("max-rust: local game: {error}");
                            ctx.status = format!("Local game: {error}");
                            // Старт не удался (например, нет данных планеты для
                            // фаз) — возвращаемся в меню, а не остаёмся на
                            // пустом экране PreGame.
                            screen = Screen::MainMenu;
                        }
                    }
                }
                Transition::StartSession(request) => {
                    pending = Some(request);
                    break;
                }
            }
        }

        // M-SP: тик локальной партии, таймер хода и ходы ИИ.
        let frame_dt = last_frame.elapsed().as_secs_f32().clamp(0.0, 0.25);
        last_frame = std::time::Instant::now();

        if matches!(screen, Screen::InSession { .. }) && app.local_active() {
            let update = app.local_update(frame_dt);

            if let Some(local) = app.local() {
                ctx.status = local.status_line(app.state().turn_counter);
            }

            if update.finished || app.local().is_some_and(|local| local.finished()) {
                screen = Screen::GameOver;
                ctx.status = "Game over — Enter/Esc returns to the menu".to_string();
            }
        }

        // Мини-карты планет для экрана настройки (`.WRL` грузятся один раз).
        if let Screen::LocalSetup(form) = &mut screen {
            let palette = app
                .assets()
                .and_then(|assets| assets.menu.as_ref())
                .map(|menu| menu.palette);
            form.ensure_minimaps(palette.as_ref());
        }

        // Курсор мыши: меню/панели — HANDPTR, карта — MAP_PTR/приказы/юниты
        // (`Cursor_SetCursor` + `GameManager_GetWindowCursor`). Без SDL-курсоров
        // (или без загруженных ресурсов) остаётся системный курсор.
        update_window_cursor(&mut window, &app, &screen, mouse);

        if let Some(request) = pending.take() {
            if runtime.is_none() {
                runtime = Some(build_ui_runtime()?);
            }
            let runtime = runtime.as_mut().expect("runtime создан выше");

            // Кадр «подключение»: пока идут Horizon/лобби, окно не пустое.
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
                    eprintln!("max-rust: UI session: {error:#}");
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
        std::thread::sleep(std::time::Duration::from_millis(16));
    }

    finish_recording(&mut recorder)?;

    Ok(())
}

/// Дописывает `.qa`-файл при выходе (хвостовой `type`/`wait`, финальный
/// комментарий) и печатает итоговый путь. Без `--record` — no-op.
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

/// GUI (запуск без флагов или `--ui`) без feature `sdl` (сборка с
/// `--no-default-features`): понятная ошибка вместо «окна нет».
#[cfg(not(feature = "sdl"))]
fn run_ui(_cli: &Cli) -> Result<()> {
    bail!(
        "GUI-режим (запуск без флагов или --ui) требует сборки с feature sdl \
         (входит в default-features): cargo run -p max_app; \
         headless-сборка — с --no-default-features"
    )
}

/// Ставит курсор окна по контексту (`Cursor_SetCursor`).
///
/// Меню/форма/список игр — `HANDPTR`; в партии — [`max_app::cursor::session_cursor`]
/// (карта/панель/юнит/приказ). Ресурсы курсоров грузятся из `MAX.RES`
/// (`CursorSet`), палитра — системная палитра окна (`FRAMEPIC`).
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

    let (cursor, frame) = if matches!(screen, Screen::InSession { .. }) {
        max_app::cursor::session_cursor(app, mouse)
    } else {
        (max_app::CursorId::Hand, 0)
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

/// Кадры сессии: ввод (мышь/клавиши) → HUD, карта + HUD → окно.
///
/// Ввод обрабатывает [`handle_session_input`]: клик по своему юниту — выбор,
/// RMB — приказ движения (пакет 38), клик по врагу — атака (08), кнопки
/// End Turn/Menu. Собранные приказы сетевой цикл забирает и отправляет сам
/// (`net::flush_hud_orders`), поэтому здесь достаточно `online = true`.
#[cfg(feature = "sdl")]
struct UiSessionSink<'a> {
    window: &'a mut max_render::SdlWindow,
    screen: Screen,
    buffer: max_render::buffer::RgbaBuffer,
    status: String,
    game_id: String,
    /// Слой 2.5 QA: тот же поток событий, что уходит в HUD.
    recorder: Option<&'a mut Recorder>,
}

#[cfg(feature = "sdl")]
impl max_app::net::FrameSink for UiSessionSink<'_> {
    fn frame(&mut self, app: &mut App) -> bool {
        app.update_visual_units(1.0 / 60.0);

        // 1. Ввод этого кадра → HUD (выделение, приказы, кнопки).
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

            match handle_session_input(app, input, true) {
                Transition::None => {}
                Transition::LeaveSession | Transition::Quit => leave = true,
                Transition::StartSession(_) | Transition::StartLocal(_) => {}
            }
        }

        if !running || leave {
            return false;
        }

        // 2. Кадр: карта + HUD поверх неё.
        self.status = format!(
            "IN SESSION · {} · tick={} turn={} · LMB select · RMB move · Esc cancel",
            self.game_id,
            app.ticks(),
            app.state().turn_counter
        );

        let view = ScreenView {
            width: self.buffer.width(),
            height: self.buffer.height(),
            status: &self.status,
            mouse: None,
            focus: 0,
            app: Some(app),
        };
        self.screen.render(&mut self.buffer, view);

        if let Err(error) = self.window.present(&self.buffer) {
            eprintln!("SDL: {error}");
            return false;
        }

        true
    }

    /// Строка «сессия разогревается… попытка N» поверх окна: пока
    /// `SessionHandle::start` повторяется, кадров партии ещё нет.
    fn join_status(&mut self, message: &str) -> bool {
        self.status = message.to_string();
        let view = ScreenView {
            width: self.buffer.width(),
            height: self.buffer.height(),
            status: &self.status,
            mouse: None,
            focus: 0,
            app: None,
        };
        self.screen.render(&mut self.buffer, view);

        if let Err(error) = self.window.present(&self.buffer) {
            eprintln!("SDL: {error}");
            return false;
        }

        true
    }
}

// ---------------------------------------------------------------------------
// Разбор значений
// ---------------------------------------------------------------------------

/// `--scale`: `None`/`0` — подобрать под рабочий стол, иначе 1..=8.
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
max-rust — клиент M.A.X. (core + net + assets + render)

ИСПОЛЬЗОВАНИЕ:
    max-rust.exe [ОПЦИИ]                     # по умолчанию — GUI (окно SDL3)
    max-rust.exe --demo <save> [--turns N] [ОПЦИИ]
    max-rust.exe --new-game [ОПЦИИ]
    max-rust.exe --connect <host[:port]> --join <game_id> [--sdl] [ОПЦИИ]

РЕЖИМЫ:
    (без флагов)         интерактивное меню: создать/выбрать игру и играть
                         (GUI по умолчанию, feature sdl в default-features;
                         то же, что --ui)
    --ui                 совместимый alias для запуска по умолчанию (GUI)
    --demo <save>        V70-сейв → N тиков → digest (headless)
    --new-game           локальная партия (setup::generate_map) + кадр
    --connect <host>     Horizon gRPC (порт по умолчанию {}) + ENet-сессия
    --join <game_id>     игра для --connect
    --sdl                окно SDL3 для --connect (feature sdl — в default)
    --record <file.qa>   записать ручную игру в QA-сценарий (GUI; --ui не нужен;
                         существующий файл не перезаписывается — суффикс -1;
                         без SDL запись доступна как API max_app::qa::Recorder)

ИГРА:
    --turns N            тиков в --demo (5) / лимит ходов в --connect (0 — без лимита)
    --reconnect-attempts N   попыток переподключения при разрыве ENet (3; 0 — выкл.)
    --reconnect-timeout S    таймаут одной попытки переподключения в секундах (10)
    --join-retry-attempts N  попыток первого подключения после Create game (6;
                             0 — одна попытка без ретраев)
    --join-retry-timeout S   таймаут одной попытки подключения в секундах (4);
                             backoff 1,2,3,5,8,11 с, окно ретраев ~30 с
    --turn-wait N        окно ввода в фазе PLAY сессии, секунды (0 — сразу)
    --scale N            масштаб окна: кадр 640x480 умножается на N (0/нет —
                         подобрать под рабочий стол, nearest-neighbour)
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
    max-rust.exe                                     # GUI: меню + партия
    max-rust.exe --demo max-original\\SAVE1.CAM --turns 5
    max-rust.exe --new-game --world 12 --seed 42 --out frame.ppm
    max-rust.exe --connect 127.0.0.1:8082 --join demo --sdl
    cargo run -p max_app                             # из max-rust/: GUI (default)
    cargo run -p max_app --features sound            # GUI + звук
    cargo run -p max_app --no-default-features -- --demo ...   # headless
",
        DEFAULT_GRPC_PORT
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Разбор без `std::env::args` и без проверки feature `sdl`.
    fn parse(args: &[&str]) -> Result<Cli> {
        Cli::parse_from(args.iter().map(|arg| (*arg).to_string()))
    }

    #[test]
    fn no_flags_selects_gui_mode() {
        let cli = parse(&[]).expect("пустой список аргументов разбирается");

        assert_eq!(cli.mode, Mode::Ui);
        assert_eq!(cli.config.mode, AppMode::Ui);
    }

    #[test]
    fn ui_flag_is_a_compatible_alias_for_the_default() {
        let cli = parse(&["--ui"]).expect("--ui разбирается");

        assert_eq!(cli.mode, Mode::Ui);
        assert_eq!(cli.config.mode, AppMode::Ui);
    }

    #[test]
    fn demo_flag_selects_headless_demo() {
        let cli = parse(&["--demo", "max-original/SAVE1.CAM", "--turns", "7"])
            .expect("--demo разбирается");

        assert_eq!(cli.mode, Mode::Demo);
        assert_eq!(cli.config.mode, AppMode::Demo);
        assert_eq!(cli.turns, Some(7));
        assert_eq!(
            cli.save.as_deref(),
            Some(std::path::Path::new("max-original/SAVE1.CAM"))
        );
    }

    #[test]
    fn new_game_flag_selects_headless_new_game() {
        let cli = parse(&["--new-game", "--world", "12"]).expect("--new-game разбирается");

        assert_eq!(cli.mode, Mode::NewGame);
        assert_eq!(cli.config.mode, AppMode::NewGame);
    }

    #[test]
    fn connect_flag_selects_headless_connect() {
        let cli = parse(&["--connect", "127.0.0.1:8082", "--join", "demo"])
            .expect("--connect разбирается");

        assert_eq!(cli.mode, Mode::Connect);
        assert_eq!(cli.config.mode, AppMode::Connect);
        assert_eq!(cli.config.horizon_host, "127.0.0.1");
        assert_eq!(cli.config.horizon_port, 8082);
        assert_eq!(cli.game_id.as_deref(), Some("demo"));
        assert!(!cli.sdl, "без --sdl сессия остаётся headless");
    }

    #[test]
    fn record_is_accepted_in_the_default_gui_mode() {
        let cli = parse(&["--record", "run.qa"]).expect("--record в GUI разбирается");

        assert_eq!(cli.mode, Mode::Ui);
        assert_eq!(cli.record.as_deref(), Some(std::path::Path::new("run.qa")));
    }

    #[test]
    fn record_is_rejected_in_headless_modes() {
        let error = parse(&["--demo", "save.cam", "--record", "run.qa"])
            .expect_err("--record с --demo должен отклоняться");

        assert!(error.to_string().contains("--record"), "{error:#}");
    }

    #[test]
    fn sdl_still_requires_connect() {
        let error = parse(&["--sdl"]).expect_err("--sdl без --connect отклоняется");

        assert!(error.to_string().contains("--connect"), "{error:#}");
    }

    #[test]
    fn unknown_flag_is_rejected() {
        let error = parse(&["--bogus"]).expect_err("неизвестный флаг отклоняется");

        assert!(error.to_string().contains("--bogus"), "{error:#}");
    }

    #[cfg(not(feature = "sdl"))]
    #[test]
    fn gui_without_sdl_feature_reports_a_build_hint() {
        let cli = parse(&[]).expect("аргументы разбираются без SDL");

        let error = cli
            .ensure_gui_available()
            .expect_err("без feature sdl GUI недоступен");
        let text = format!("{error:#}");

        assert!(text.contains("feature sdl"), "{text}");
        assert!(text.contains("--demo"), "{text}");
        assert!(text.contains("--new-game"), "{text}");
        assert!(text.contains("--connect"), "{text}");
    }

    #[cfg(feature = "sdl")]
    #[test]
    fn gui_is_available_with_sdl_feature() {
        parse(&[])
            .expect("аргументы разбираются")
            .ensure_gui_available()
            .expect("с feature sdl GUI доступен");
    }
}
