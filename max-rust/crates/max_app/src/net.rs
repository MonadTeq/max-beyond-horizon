//! Сетевой режим: Horizon (gRPC) → ENet-сессия → внутриигровое лобби → игра.
//!
//! Последовательность соответствует `docs/protocol.md` §2/§7/§8:
//!
//! 1. [`connect_horizon`] — `Hello` + `JoinGame`; ответ [`GameJoined`] несёт
//!    `host`/`port`/`ticket` UDP-сессии (`docs/protocol-grpc.md` §4).
//! 2. [`SessionConfig::from_joined`] + [`SessionHandle::start`] — ENet-подключение
//!    и `TP 00`/`TP 03` (канал 0).
//! 3. Внутриигровое лобби (канал 1): `28 → 44 → 29 → 30 → 36 → 35 → 32 → 05 → 34`
//!    (§8.2). Эту часть выполняет [`run_lobby`].
//! 4. Игровой цикл ([`run_network_with_sink`]): мир/seed/типы команд из
//!    `LobbyGameSettings` (пакет 30), затем FSM хода
//!    `00 → 53 → тики → 06 → 52 → 54 → 45 → finish_turn`
//!    (`docs/barrier-flow.md` §3) → [`max_core::game::GameState::finish_turn`].
//!
//! # Барьеры
//!
//! Сервер-арбитр не эхо-шлёт барьеры клиента: барьер завершён, когда **каждая
//! удалённая команда** прислала счётчик, равный своему (`Remote_Wait*`,
//! `remote.cpp`). Поэтому:
//!
//! * [`TurnBarrierFsm`] держит отдельные счётчики 00/52/54/45, все
//!   pre-increment (первый отчёт — **1**, как `++Remote_FrameSyncCounter` в C++);
//! * каждый барьер реально ожидается (`wait_barrier`) с повтором через 3 с и
//!   таймаутом 30 с; после прохода счётчики/трекер очищаются;
//! * повторы идут через [`SessionHandle::send_untracked`], чтобы сессионный
//!   трекер не получал вторую запись на каждый ретрансмит
//!   (`docs/barrier-flow.md` §6.8);
//! * входящий 54 применяется к [`max_core::game::GameState`]
//!   (`max_core::events::apply_state_sync`), после 45-барьера CRC команды
//!   сверяются с собственным (`Remote_TeamDataCrc16`).

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use max_core::enums::{GameStateType, PlayMode, Planet, VictoryType};
use max_core::events::apply_state_sync;
use max_core::geo::Point;
use max_core::game::{GameState, PLAYER_TEAM_COUNT};
use max_core::input::{
    Command, Packet00TurnBarrier, Packet01FrameBarrier, Packet06FinishedTurn,
    Packet52EndTurnBarrier,
};
use max_core::setup::{ResourceLevel, TeamStart};
use max_core::team::{TeamId, TeamKind};
use max_net::handshake::DataHash;
use max_net::session::{DEFAULT_BARRIER_REPEAT, DEFAULT_BARRIER_TIMEOUT};
use max_net::{
    complex_summary_for, crc_barrier_for, state_sync_for, GameJoined, HorizonClient,
    LobbyGameSettings, LobbyPacket, Packet, SessionConfig, SessionError, SessionEvent, SessionHandle,
    CLIENT_NAME, HORIZON_PROTOCOL_VERSION,
};
use tokio::time::Instant;

use crate::{App, AppConfig, AppDigest, NewGameConfig, WORLD_ASSETS};

/// Сколько ждать одно событие сессии, прежде чем тикать симуляцию.
pub const DEFAULT_EVENT_WAIT: Duration = Duration::from_millis(2);
/// Пауза между тиками сетевого цикла (клиент не должен крутить CPU в 100 %).
pub const DEFAULT_TICK_INTERVAL: Duration = Duration::from_millis(20);
/// Сколько ждать полного лобби (28 → 34).
pub const DEFAULT_LOBBY_TIMEOUT: Duration = Duration::from_secs(30);

/// Параметры сетевого режима (`--connect`).
#[derive(Debug, Clone)]
pub struct NetworkOptions {
    /// `game_id` из лобби Horizon (`--join`).
    pub game_id: String,
    /// Пароль игры (опционально).
    pub password: Option<String>,
    /// Сколько ходов сыграть; `0` — до разрыва/остановки.
    pub max_turns: u32,
    /// Таймаут лобби.
    pub lobby_timeout: Duration,
    /// Пауза между тиками.
    pub tick_interval: Duration,
    /// Ожидание события сессии за итерацию.
    pub event_wait: Duration,
}

impl Default for NetworkOptions {
    fn default() -> Self {
        Self {
            game_id: String::new(),
            password: None,
            max_turns: 0,
            lobby_timeout: DEFAULT_LOBBY_TIMEOUT,
            tick_interval: DEFAULT_TICK_INTERVAL,
            event_wait: DEFAULT_EVENT_WAIT,
        }
    }
}

impl NetworkOptions {
    /// Параметры для игры `game_id` и таймаутами по умолчанию.
    pub fn new(game_id: impl Into<String>) -> Self {
        Self {
            game_id: game_id.into(),
            ..Self::default()
        }
    }
}

/// Приёмник кадров сетевого цикла.
///
/// Библиотека не знает про SDL: бинарник под feature `sdl` передаёт сюда
/// обёртку над [`max_render::SdlWindow`], тесты — счётчик кадров. `false`
/// означает «закрыть окно» и корректно завершить сессию.
///
/// Кадр получает `&mut App`, потому что интерактивный клиент (`--ui`) здесь же
/// обрабатывает мышь: выделение юнита и приказы живут в
/// [`crate::hud::HudState`] (вне `GameState`), а собранные пакеты `38`/`08`/`18`
/// забирает и отправляет [`flush_hud_orders`].
pub trait FrameSink {
    /// Показать кадр текущего состояния; `false` — выйти из цикла.
    fn frame(&mut self, app: &mut App) -> bool;
}

/// Результат сетевого прогона (`--connect`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkReport {
    /// `game_id` сессии.
    pub game_id: String,
    /// `TP 00` прошёл.
    pub handshake: bool,
    /// Лобби дошло до пакета 34.
    pub lobby_complete: bool,
    /// Локальная команда (слот из [`AppConfig::team_slot`]).
    pub local_team: TeamId,
    /// Сколько тиков симуляции прокручено.
    pub ticks: u64,
    /// Сколько команд применено.
    pub commands_applied: u64,
    /// Сколько ходов закрыто барьером 52.
    pub turns_finished: u32,
    /// Сколько раз барьер не дождался пиров.
    pub barrier_timeouts: u32,
    /// Дайджест итогового состояния.
    pub digest: AppDigest,
}

/// Подключается к Horizon и входит в игру (`Hello` → `JoinGame`).
///
/// Возвращает [`GameJoined`]: адрес UDP-сессии, тикет и `game_id`.
pub async fn connect_horizon(
    config: &AppConfig,
    game_id: &str,
    password: Option<&str>,
) -> Result<GameJoined> {
    let mut client = HorizonClient::connect(&config.horizon_host, config.horizon_port)
        .await
        .with_context(|| {
            format!(
                "Horizon gRPC {}:{} недоступен",
                config.horizon_host, config.horizon_port
            )
        })?;

    client
        .hello(HORIZON_PROTOCOL_VERSION, CLIENT_NAME)
        .await
        .context("Horizon Hello")?;

    let joined = client
        .join_game(game_id, password)
        .await
        .with_context(|| format!("Horizon JoinGame({game_id})"))?;

    tracing::info!(
        "Horizon: игра {game_id} → UDP {}:{} (ticket {})",
        joined.host,
        joined.port,
        joined.ticket.kind_short()
    );

    Ok(joined)
}

/// Внутриигровое лобби (§8.2): возвращает идентификаторы нод и настройки.
#[derive(Debug, Clone, Default)]
struct LobbyInfo {
    handshake: bool,
    player_node: u16,
    host_node: u16,
    /// `Welcome.team_nodes` — какие ноды заняли слоты команд.
    team_nodes: [u16; 4],
    settings: Option<LobbyGameSettings>,
}

/// Проходит лобби `28 → 44 → 29 → 30 → 36 → 35 → 32 → 05 → 34`.
///
/// Пока лобби не завершено, приходящие игровые команды (например, 13 seed,
/// который сервер шлёт сразу после старта) уже применяются к [`App`] в порядке
/// получения.
async fn run_lobby(
    app: &mut App,
    handle: &mut SessionHandle,
    config: &AppConfig,
    options: &NetworkOptions,
) -> Result<LobbyInfo> {
    let deadline = Instant::now() + options.lobby_timeout;
    let mut info = LobbyInfo::default();

    handle
        .send(Packet::Lobby(LobbyPacket::EnterLobby { node: 0 }))
        .await
        .context("отправка пакета 28")?;

    loop {
        if Instant::now() >= deadline {
            bail!(
                "лобби не завершилось за {:?} (28→44→29→30→36→35→32→05→34)",
                options.lobby_timeout
            );
        }

        let event = match handle.next_event_timeout(Duration::from_millis(100)).await {
            Ok(event) => event,
            Err(SessionError::Timeout { .. }) => continue,
            Err(error) => return Err(error).context("ожидание события лобби"),
        };

        match event {
            None => bail!("сессия закрылась в лобби"),
            Some(SessionEvent::HandshakeOk) => {
                info.handshake = true;
            }
            Some(SessionEvent::Packet(Packet::Lobby(lobby))) => match lobby {
                LobbyPacket::HostAnnounce {
                    player_node,
                    game_version,
                    ..
                } => {
                    info.host_node = player_node;
                    tracing::info!(
                        "лобби: 44 host_node={player_node} game_version=0x{game_version:08X}"
                    );
                    handle
                        .send(Packet::Lobby(LobbyPacket::Register {
                            host_node: player_node,
                        }))
                        .await
                        .context("отправка пакета 29")?;
                }
                LobbyPacket::Welcome(welcome) => {
                    info.player_node = welcome.player_node;
                    info.host_node = welcome.host_node;
                    info.team_nodes = welcome.team_nodes;
                    info.settings = Some(welcome.settings.clone());

                    let team_slot = config.team_slot.min(3);
                    let team_name = format!("{}#{team_slot}", config.player_name);
                    // Свой слот известен локально: `Welcome` приходит до нашего 35.
                    if info.player_node > 4 {
                        info.team_nodes[team_slot as usize] = info.player_node;
                    }

                    handle
                        .send(Packet::Lobby(LobbyPacket::PlayerName {
                            player_node: info.player_node,
                            player_name: config.player_name.clone(),
                        }))
                        .await
                        .context("отправка пакета 36")?;
                    handle
                        .send(Packet::Lobby(LobbyPacket::TeamSelect {
                            host_node: info.host_node,
                            player_node: info.player_node,
                            team_slot,
                            ready: true,
                            team_name,
                        }))
                        .await
                        .context("отправка пакета 35")?;

                    tracing::info!(
                        "лобби: 30 player_node={} host_node={} world={} seed={} team_nodes={:?} → 36/35 (слот {team_slot}, READY)",
                        info.player_node,
                        info.host_node,
                        welcome.settings.world_name,
                        welcome.settings.rng_seed,
                        info.team_nodes
                    );
                }
                LobbyPacket::TeamSelect {
                    player_node,
                    team_slot,
                    ..
                } => {
                    // Если сервер релеит чужие 35 — обновляем карту слотов.
                    let slot = team_slot as usize;
                    if slot < info.team_nodes.len() {
                        info.team_nodes[slot] = player_node;
                        tracing::info!("лобби: 35 слот {slot} → node {player_node}");
                    }
                }
                LobbyPacket::Setup { host_node } => {
                    info.host_node = host_node;
                    handle
                        .send(Packet::Lobby(LobbyPacket::SetupAck { host_node }))
                        .await
                        .context("отправка пакета 05")?;
                    tracing::info!("лобби: 32 setup → 05 ack");
                }
                LobbyPacket::Start { host_node } => {
                    info.host_node = host_node;
                    tracing::info!("лобби: 34 start (host_node={host_node})");
                    return Ok(info);
                }
                other => tracing::debug!("лобби: пакет {other:?}"),
            },
            Some(SessionEvent::Packet(Packet::Command(command))) => {
                app.apply(command);
            }
            Some(SessionEvent::Packet(packet)) => {
                tracing::debug!("лобби: пакет {packet:?}");
            }
            Some(SessionEvent::HandshakeRejected {
                reason,
                reason_text,
            }) => bail!("хендшейк отклонён: {:?} ({reason_text})", reason),
            Some(SessionEvent::Disconnected) => bail!("сервер отключил клиента в лобби"),
            Some(SessionEvent::Error(error)) => {
                return Err(error).context("ошибка сессии в лобби");
            }
            Some(SessionEvent::Malformed { error }) => {
                tracing::warn!("лобби: битый пакет: {error}");
            }
            Some(SessionEvent::BarrierDue { .. }) | Some(SessionEvent::BarrierTimeout { .. }) => {}
        }
    }
}

/// Сетевой прогон без окна: [`run_network_with_sink`] с `sink = None`.
pub async fn run_network(app: &mut App, options: &NetworkOptions) -> Result<NetworkReport> {
    run_network_with_sink(app, options, None).await
}

/// Полный сетевой режим: Horizon → ENet → лобби → игровой цикл.
///
/// `sink` получает кадр после каждого хода (SDL-окно в бинарнике). Если
/// [`FrameSink::frame`] вернул `false`, цикл завершается и сессия закрывается
/// штатно.
pub async fn run_network_with_sink(
    app: &mut App,
    options: &NetworkOptions,
    mut sink: Option<&mut dyn FrameSink>,
) -> Result<NetworkReport> {
    let config = app.config().clone();
    // Хэндшейк считает PATCHES.RES по своему пути: серверный `game_data` может
    // не содержать файла, и тогда его часть хэша пуста. Ассеты грузятся
    // отдельно (`App::init_assets` → `assets::resolve_patches_res`), поэтому
    // несуществующий `--handshake-patches` не оставляет игру без карты.
    let handshake_patches = config.handshake_patches_path().to_path_buf();
    let data_hash = DataHash::from_files(&config.max_res, &handshake_patches).with_context(|| {
        format!(
            "не удалось посчитать SHA256({} + {})",
            config.max_res.display(),
            handshake_patches.display()
        )
    })?;

    let joined = connect_horizon(&config, &options.game_id, options.password.as_deref()).await?;
    let session_config = SessionConfig::from_joined(&joined, data_hash).with_player(
        &config.player_name,
        config.team_slot.min(3),
        config.clan,
    );
    let mut handle = SessionHandle::start(session_config)
        .await
        .context("ENet-сессия: connect + TP 00")?;

    let lobby = run_lobby(app, &mut handle, &config, options).await?;

    // Локальная команда: слот, который в `Welcome.team_nodes` занят нашей нодой
    // (обычно `--team`), иначе — запрошенный слот.
    let local_team = lobby
        .team_nodes
        .iter()
        .position(|&node| node > 4 && node == lobby.player_node)
        .map(|slot| TeamId(slot as u8))
        .unwrap_or(TeamId(config.team_slot.min(3)));

    initialize_game(app, &lobby, local_team);

    tracing::info!(
        "сессия готова: game={} local_team={} лобби={:?}",
        joined.game_id,
        local_team.0,
        lobby
            .settings
            .as_ref()
            .map(|settings| settings.world_name.clone())
    );

    let stats = run_game_loop(app, &mut handle, local_team, options, &mut sink).await?;
    handle.shutdown().await;

    Ok(NetworkReport {
        game_id: joined.game_id,
        handshake: lobby.handshake,
        lobby_complete: true,
        local_team,
        ticks: app.ticks(),
        commands_applied: app.applied_commands(),
        turns_finished: stats.turns_finished,
        barrier_timeouts: stats.barrier_timeouts,
        digest: app.digest(),
    })
}

/// Инициализирует мир, seed и типы команд из `LobbyGameSettings` (пакет 30).
///
/// Порт той части `GameManager_InitGame`/`Remote_SetupPlayers`, без которой
/// CRC-барьер 45 сравнивает пустую карту с серверной и «честно» показывает
/// десинк:
///
/// * `team_nodes[team] > 4` → своя нода = [`TeamKind::Player`], чужая =
///   [`TeamKind::Remote`], незанятый слот = [`TeamKind::None`]
///   (`Remote_SetupPlayers`, `remote.cpp:672-734`);
/// * `world`/`seed`/уровни ресурсов и `ini_*` — из настроек лобби; карта
///   генерируется `setup::generate_map` (детерминированно, как на сервере);
/// * `ini_play_mode` решает, нужен ли 00-барьер каждый ход.
fn initialize_game(app: &mut App, lobby: &LobbyInfo, local_team: TeamId) {
    let Some(settings) = lobby.settings.as_ref() else {
        return;
    };

    let mut kinds = team_kinds_from_nodes(lobby.player_node, &lobby.team_nodes);
    if let Some(slot) = kinds.get_mut(local_team.0 as usize) {
        *slot = TeamKind::Player;
    }

    let planet = planet_from_settings(settings).unwrap_or(app.config().planet);
    let level = |raw: i32| {
        ResourceLevel::new(raw.clamp(0, 2) as u8).unwrap_or(ResourceLevel::NORMAL)
    };

    // Команды, пришедшие в лобби (12 — снабжение миссии), относятся к «старому»
    // состоянию; после генерации мира их нужно вернуть, иначе позиции команд
    // потеряются. Точка старта из 12 (если уже пришла) идёт в генерацию.
    let supplies = app.state().mission_supplies.clone();

    let teams = (0..PLAYER_TEAM_COUNT)
        .filter(|&team| kinds[team] != TeamKind::None)
        .map(|team| {
            let position = supplies
                .get(team)
                .and_then(|supply| supply.as_ref())
                .map(|supply| supply.starting_position)
                .unwrap_or_else(|| default_team_start(team));
            TeamStart::new(TeamId(team as u8), position)
        })
        .collect();
    let config = NewGameConfig {
        planet,
        seed: settings.rng_seed,
        raw: level(settings.ini_raw_resource),
        fuel: level(settings.ini_fuel_resource),
        gold: level(settings.ini_gold_resource),
        derelicts: level(settings.ini_alien_derelicts),
        teams,
        player_team: local_team,
        start_gold: settings.ini_start_gold.max(0) as u32,
    };

    match app.new_game(&config) {
        Ok(report) => tracing::info!(
            "мир из пакета 30: planet={:?} seed={} teams={} units={} start_units={} aliens={}",
            report.planet,
            report.seed,
            report.teams,
            report.units,
            report.start_units,
            report.aliens
        ),
        Err(error) => {
            tracing::warn!(
                "не удалось сгенерировать мир из настроек лобби ({error:#}); остаётся пустая карта"
            );
            app.state_mut().rng.set_seed(settings.rng_seed);
        }
    }

    let play_mode = PlayMode::try_from(u8::try_from(settings.ini_play_mode).unwrap_or(0))
        .unwrap_or(PlayMode::TurnBased);
    let state = app.state_mut();
    state.player_team = local_team;
    state.play_mode = play_mode;
    state.mission_category = settings.mission_category;
    if let Ok(victory_type) =
        VictoryType::try_from(u8::try_from(settings.ini_victory_type).unwrap_or(0))
    {
        state.victory_type = victory_type;
    }
    if settings.ini_victory_limit > 0 {
        state.victory_limit = settings.ini_victory_limit;
    }

    for team in 0..PLAYER_TEAM_COUNT {
        state.teams[team].team_type = kinds[team];
        if kinds[team] != TeamKind::None {
            state.team_units[team].gold = settings.ini_start_gold.max(0) as u32;
        }
    }
    state.mission_supplies = supplies;

    tracing::info!(
        "игра из лобби: local_team={} play_mode={:?} victory_limit={} team_types={:?}",
        local_team.0,
        play_mode,
        state.victory_limit,
        kinds
    );
}

/// `Remote_SetupPlayers` (`remote.cpp:705-729`): тип команды по её ноде.
///
/// `team_nodes[team] > 4` — слот занят клиентом; своя нода → `Player`, чужая →
/// `Remote`. Незанятый слот остаётся `None` (AI-команды сервера клиент не
/// симулирует и в барьерах не ждёт).
fn team_kinds_from_nodes(
    player_node: u16,
    team_nodes: &[u16; 4],
) -> [TeamKind; PLAYER_TEAM_COUNT] {
    std::array::from_fn(|team| {
        let node = team_nodes.get(team).copied().unwrap_or(0);
        if node > 4 {
            if node == player_node {
                TeamKind::Player
            } else {
                TeamKind::Remote
            }
        } else {
            TeamKind::None
        }
    })
}

/// Планета из настроек лобби: `ini_world_index`, иначе имя `.WRL`-ресурса.
fn planet_from_settings(settings: &LobbyGameSettings) -> Option<Planet> {
    if let Ok(index) = u8::try_from(settings.ini_world_index) {
        if let Ok(planet) = Planet::try_from(index) {
            return Some(planet);
        }
    }

    let name = settings.world_name.as_str();
    let name = name.trim();
    if name.is_empty() {
        return None;
    }

    WORLD_ASSETS
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(name))
        .and_then(|index| Planet::try_from(index as u8).ok())
}

/// Стартовая клетка команды по умолчанию (до пакета 12 с точной позицией).
fn default_team_start(team: usize) -> Point {
    const STARTS: [(i16, i16); 4] = [(20, 20), (90, 20), (20, 90), (90, 90)];
    let (x, y) = STARTS[team % STARTS.len()];
    Point::new(x, y)
}

#[derive(Debug, Default)]
struct GameLoopStats {
    turns_finished: u32,
    barrier_timeouts: u32,
}

/// Повтор барьера (`REMOTE_PING_TIME_PERIOD`).
const BARRIER_RETRY: Duration = DEFAULT_BARRIER_REPEAT;
/// Таймаут ожидания пиров (`REMOTE_RESPONSE_TIMEOUT`).
const BARRIER_TIMEOUT: Duration = DEFAULT_BARRIER_TIMEOUT;

/// Фаза барьерного FSM текущего хода (`docs/barrier-flow.md` §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarrierPhase {
    /// 00 — вход в раунд.
    Begin,
    /// 53 — итоги баз на начало хода.
    Play,
    /// 52 — конец хода.
    End,
    /// 54 — синхронизация состояния команды.
    Sync,
    /// 45 — CRC-барьер.
    Crc,
}

impl BarrierPhase {
    const fn name(self) -> &'static str {
        match self {
            Self::Begin => "00",
            Self::Play => "53",
            Self::End => "52",
            Self::Sync => "54",
            Self::Crc => "45",
        }
    }
}

/// Клиентский FSM барьеров хода: `00 → 53 → 06 → 52 → 54 → 45`.
///
/// Счётчики — отдельные (`Remote_FrameSyncCounter`, `Remote_TurnIndices`,
/// `Remote_StateSyncIndex`, `Remote_NextTurnIndices`), все pre-increment: первый
/// отчёт равен **1**. `GameManager_TurnCounter` в C++ стартует с 1, а
/// `GameState::turn_counter` — с 0, поэтому 53 несёт `turn_counter + 1`.
#[derive(Debug)]
struct TurnBarrierFsm {
    local_team: TeamId,
    /// В `SimultaneousMoves` 00-барьер нужен каждый ход, в `TurnBased` — один
    /// раз на старте (`game_manager.cpp:496`).
    simultaneous: bool,
    phase: BarrierPhase,
    begin_counter: u8,
    end_counter: u8,
    sync_index: u8,
    crc_index: u8,
    /// `GameManager_TurnCounter` для 53 (1-based).
    complex_turn: i32,
    /// Какой `turn` последней 53 прислала каждая команда.
    received_complex: [Option<i32>; PLAYER_TEAM_COUNT],
    /// CRC собственного мира в текущем 45-раунде.
    crc_checksum: u16,
    /// 06 отправлен один раз за ход.
    local_finished_sent: bool,
}

impl TurnBarrierFsm {
    fn new(local_team: TeamId, play_mode: PlayMode) -> Self {
        Self {
            local_team,
            simultaneous: play_mode == PlayMode::SimultaneousMoves,
            phase: BarrierPhase::Begin,
            begin_counter: 0,
            end_counter: 0,
            sync_index: 0,
            crc_index: 0,
            complex_turn: 0,
            received_complex: [None; PLAYER_TEAM_COUNT],
            crc_checksum: 0,
            local_finished_sent: false,
        }
    }

    /// C++-номер текущего хода (`GameManager_TurnCounter`, с 1).
    fn cxx_turn(state: &GameState) -> i32 {
        state.turn_counter + 1
    }

    /// Команды-пиры: те, чьи барьеры нужно ждать (`TEAM_TYPE_REMOTE`).
    fn remote_teams<'a>(
        &'a self,
        state: &'a GameState,
    ) -> impl Iterator<Item = usize> + 'a {
        (0..PLAYER_TEAM_COUNT)
            .filter(move |&team| state.teams[team].team_type == TeamKind::Remote)
    }

    /// `Remote_WaitBeginTurnAcknowledge`: `++Remote_FrameSyncCounter[own]` → 00.
    fn begin(&mut self, state: &mut GameState) -> Vec<Packet> {
        let slot = self.local_team.0 as usize;
        let counter = state.frame_sync_counter[slot].wrapping_add(1);
        state.frame_sync_counter[slot] = counter;
        self.begin_counter = counter;
        self.phase = BarrierPhase::Begin;

        vec![Packet::Command(Command::TurnBarrier(Packet00TurnBarrier {
            team: self.local_team,
            counter,
        }))]
    }

    fn begin_reached(&self, state: &GameState) -> bool {
        self.remote_teams(state)
            .all(|team| state.frame_sync_counter[team] == self.begin_counter)
    }

    /// `Remote_WaitComplexTotals`: 53 своей команды за текущий ход.
    fn complex(&mut self, state: &GameState) -> Vec<Packet> {
        self.complex_turn = Self::cxx_turn(state);
        self.phase = BarrierPhase::Play;

        vec![complex_summary_for(
            state,
            self.local_team,
            self.complex_turn,
        )]
    }

    /// Запоминает входящую 53 (вызывается для всех команд, включая свою).
    fn note_complex(&mut self, team: TeamId, turn: i32) {
        if let Some(slot) = self.received_complex.get_mut(team.0 as usize) {
            *slot = Some(turn);
        }
    }

    fn complex_reached(&self, state: &GameState) -> bool {
        self.remote_teams(state)
            .all(|team| self.received_complex[team] == Some(self.complex_turn))
    }

    /// `Remote_WaitEndTurnAcknowledge`: `++Remote_TurnIndices[own]` → 52.
    fn end(&mut self, state: &mut GameState) -> Vec<Packet> {
        let slot = self.local_team.0 as usize;
        let counter = state.turn_indices[slot].wrapping_add(1);
        state.turn_indices[slot] = counter;
        self.end_counter = counter;
        self.phase = BarrierPhase::End;

        vec![Packet::Command(Command::EndTurnBarrier(
            Packet52EndTurnBarrier {
                team: self.local_team,
                counter,
            },
        ))]
    }

    fn end_reached(&self, state: &GameState) -> bool {
        self.remote_teams(state)
            .all(|team| state.turn_indices[team] == self.end_counter)
    }

    /// `Remote_SynchronizeUnitStates`: `++Remote_StateSyncIndex` → 54.
    fn sync(&mut self, state: &GameState) -> Vec<Packet> {
        self.sync_index = self.sync_index.wrapping_add(1);
        self.phase = BarrierPhase::Sync;

        state_sync_for(state, self.local_team, self.sync_index)
            .into_iter()
            .map(Packet::StateSync)
            .collect()
    }

    fn sync_reached(&self, state: &GameState) -> bool {
        self.remote_teams(state)
            .all(|team| state.state_sync_indices[team] == self.sync_index)
    }

    /// `Remote_CheckDesync`: `++Remote_NextTurnIndices[own]` → 45.
    fn crc(&mut self, state: &mut GameState) -> Vec<Packet> {
        let slot = self.local_team.0 as usize;
        let index = state.next_turn_indices[slot].wrapping_add(1);
        state.next_turn_indices[slot] = index;
        self.crc_index = index;
        self.crc_checksum = max_core::crc::state_checksum(state);
        self.phase = BarrierPhase::Crc;

        vec![crc_barrier_for(self.local_team, index, state)]
    }

    fn crc_reached(&self, state: &GameState) -> bool {
        self.remote_teams(state)
            .all(|team| state.next_turn_indices[team] == self.crc_index)
    }

    /// Сверка CRC после 45-барьера: у всех пиров тот же `crc16`.
    fn crc_matches(&self, state: &GameState) -> bool {
        self.remote_teams(state)
            .all(|team| state.team_data_crc16[team] == self.crc_checksum)
    }

    /// Текущая фаза завершена (все пиры прислали ожидаемое значение).
    fn reached(&self, state: &GameState) -> bool {
        match self.phase {
            BarrierPhase::Begin => self.begin_reached(state),
            BarrierPhase::Play => self.complex_reached(state),
            BarrierPhase::End => self.end_reached(state),
            BarrierPhase::Sync => self.sync_reached(state),
            BarrierPhase::Crc => self.crc_reached(state),
        }
    }

    fn mark_local_finished(&mut self) {
        self.local_finished_sent = true;
    }

    fn local_finished_sent(&self) -> bool {
        self.local_finished_sent
    }

    /// Ход закрыт (`GameManager_TeamTurnFinish`): сброс локального флага,
    /// следующий 00 — только в `SimultaneousMoves`.
    fn on_turn_finished(&mut self) {
        self.local_finished_sent = false;
        self.phase = if self.simultaneous {
            BarrierPhase::Begin
        } else {
            BarrierPhase::Play
        };
    }
}

/// Исход ожидания барьера.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitOutcome {
    /// Барьер собран.
    Reached,
    /// Сессия завершилась.
    Stopped,
}

/// Отложенная отправка барьера: повтор каждые 3 с, лог таймаута через 30 с.
///
/// В отличие от [`max_net::BarrierTracker`] запись **не** исчезает после
/// таймаута: C++ продолжает пинговать отстающую команду, Rust логирует и ждёт
/// (`docs/barrier-flow.md` §3).
struct BarrierPending {
    packets: Vec<Packet>,
    first_sent: Instant,
    last_sent: Instant,
    attempts: u32,
    timeout_reported: bool,
}

impl BarrierPending {
    fn new(packets: Vec<Packet>, now: Instant) -> Self {
        Self {
            packets,
            first_sent: now,
            last_sent: now,
            attempts: 0,
            timeout_reported: false,
        }
    }

    fn poll(&mut self, now: Instant) -> Option<BarrierPoll> {
        if !self.timeout_reported && now.duration_since(self.first_sent) >= BARRIER_TIMEOUT {
            self.timeout_reported = true;
            return Some(BarrierPoll::TimedOut);
        }
        if now.duration_since(self.last_sent) >= BARRIER_RETRY {
            self.last_sent = now;
            self.attempts = self.attempts.saturating_add(1);
            return Some(BarrierPoll::Retransmit);
        }
        None
    }
}

enum BarrierPoll {
    Retransmit,
    TimedOut,
}

/// Вычерпывает события сессии; `false` — сессия/цикл завершены.
async fn pump_events(
    app: &mut App,
    handle: &mut SessionHandle,
    stats: &mut GameLoopStats,
    fsm: &mut TurnBarrierFsm,
    options: &NetworkOptions,
) -> Result<bool> {
    loop {
        match handle.next_event_timeout(options.event_wait).await {
            Ok(Some(event)) => {
                if !handle_session_event(app, event, stats, fsm)? {
                    return Ok(false);
                }
            }
            Ok(None) => return Ok(false),
            Err(SessionError::Timeout { .. }) => return Ok(true),
            Err(error) => return Err(error).context("ожидание события сессии"),
        }
    }
}

/// Отправляет пакеты барьера без регистрации в трекере сессии (повторы делает
/// [`BarrierPending`], поэтому двойного трекинга нет).
async fn send_barrier(handle: &SessionHandle, packets: &[Packet]) -> Result<()> {
    for packet in packets {
        handle
            .send_untracked(packet.clone())
            .await
            .context("отправка барьера")?;
    }
    Ok(())
}

/// Отправляет приказы/чат, собранные вводом HUD (M4.3/M4.4), в сессию.
///
/// Порядок как в C++: сначала локальное применение (`Remote_ReceiveNetPacket_*`
/// для собственной команды), затем multicast — `Packet::Command` через
/// [`SessionHandle::send`]. Пакеты 38/08/18 барьерами не являются, поэтому
/// трекер повторов сессии их не регистрирует (там только 00/45/52).
///
/// Источник — [`crate::hud::HudState::pending`]: их собирает
/// [`crate::ui::handle_session_input`] из кликов (38 — движение с путём,
/// 08 — атака) и из строки ввода чата (18 — `Command::Chat`).
async fn flush_hud_orders(app: &mut App, handle: &SessionHandle) -> Result<()> {
    for command in app.take_outgoing_orders() {
        let kind = format!("{:?}", command.kind());

        app.apply(command.clone());
        handle
            .send(Packet::Command(command))
            .await
            .with_context(|| format!("отправка приказа HUD ({kind})"))?;
        tracing::debug!("HUD: приказ {kind} применён локально и отправлен в сессию");
    }

    Ok(())
}

/// Покадровый барьер 01 (real-time pacing, §8.6).
async fn send_frame_barrier(
    app: &mut App,
    handle: &SessionHandle,
    local_team: TeamId,
    frame_counter: &mut u8,
) -> Result<()> {
    *frame_counter = frame_counter.wrapping_add(1) & 0x3F;
    let command = Command::FrameBarrier(Packet01FrameBarrier {
        team: local_team,
        counter: *frame_counter,
    });
    app.apply(command.clone());
    handle
        .send_untracked(Packet::Command(command))
        .await
        .context("отправка барьера 01")
}

/// Ждёт, пока все пиры пришлют значения текущего барьера.
///
/// Пока ждёт: вычерпывает события (применяя команды/54), повторяет пакеты
/// барьера через [`SessionHandle::send_untracked`] раз в 3 с, шлёт 01 и по
/// истечении 30 с логирует таймаут (но продолжает ждать, как Rust-клиент в
/// `docs/barrier-flow.md` §3).
async fn wait_barrier(
    app: &mut App,
    handle: &mut SessionHandle,
    stats: &mut GameLoopStats,
    fsm: &mut TurnBarrierFsm,
    frame_counter: &mut u8,
    options: &NetworkOptions,
    phase: BarrierPhase,
    packets: Vec<Packet>,
) -> Result<WaitOutcome> {
    let mut pending = BarrierPending::new(packets, Instant::now());

    loop {
        if !pump_events(app, handle, stats, fsm, options).await? {
            return Ok(WaitOutcome::Stopped);
        }
        if fsm.reached(app.state()) {
            return Ok(WaitOutcome::Reached);
        }

        match pending.poll(Instant::now()) {
            Some(BarrierPoll::Retransmit) => {
                tracing::debug!(
                    "барьер {}: повтор #{} ({} пакет(ов))",
                    phase.name(),
                    pending.attempts,
                    pending.packets.len()
                );
                send_barrier(handle, &pending.packets).await?;
            }
            Some(BarrierPoll::TimedOut) => {
                stats.barrier_timeouts = stats.barrier_timeouts.saturating_add(1);
                tracing::warn!(
                    "барьер {} не дождался пиров за {:?}; продолжаем ждать",
                    phase.name(),
                    BARRIER_TIMEOUT
                );
            }
            None => {}
        }

        send_frame_barrier(app, handle, fsm.local_team, frame_counter).await?;
        tokio::time::sleep(options.event_wait).await;
    }
}

/// Игровой цикл lockstep-клиента по FSM `docs/barrier-flow.md` §3.
async fn run_game_loop(
    app: &mut App,
    handle: &mut SessionHandle,
    local_team: TeamId,
    options: &NetworkOptions,
    sink: &mut Option<&mut dyn FrameSink>,
) -> Result<GameLoopStats> {
    let mut stats = GameLoopStats::default();
    let mut fsm = TurnBarrierFsm::new(local_team, app.state().play_mode);
    let mut frame_counter: u8 = 0;

    tracing::info!(
        "игровой цикл: local_team={} play_mode={:?} пиров={}",
        local_team.0,
        app.state().play_mode,
        (0..PLAYER_TEAM_COUNT)
            .filter(|&team| app.state().teams[team].team_type == TeamKind::Remote)
            .count()
    );

    // BEGIN (00): загрузка партии и каждый ход в SimultaneousMoves.
    let packets = fsm.begin(app.state_mut());
    send_barrier(handle, &packets).await?;
    if wait_barrier(
        app,
        handle,
        &mut stats,
        &mut fsm,
        &mut frame_counter,
        options,
        BarrierPhase::Begin,
        packets,
    )
    .await?
        == WaitOutcome::Stopped
    {
        return Ok(stats);
    }
    handle.clear_barriers().await?;
    tracing::info!("барьер 00 (начало хода) пройден");

    loop {
        // PLAY (53): итоги баз на начало хода, затем один тик и 06.
        let packets = fsm.complex(app.state());
        send_barrier(handle, &packets).await?;
        if wait_barrier(
            app,
            handle,
            &mut stats,
            &mut fsm,
            &mut frame_counter,
            options,
            BarrierPhase::Play,
            packets,
        )
        .await?
            == WaitOutcome::Stopped
        {
            return Ok(stats);
        }
        handle.clear_barriers().await?;
        tracing::debug!(
            "барьер 53 (итоги баз) пройден: turn={}",
            fsm.complex_turn
        );

        // Тики хода: у headless-клиента нет локального ввода, поэтому ход
        // закрывается сразу после обработки очереди пришедших команд
        // (`Access_AreTaskEventsPending` == false).
        if !pump_events(app, handle, &mut stats, &mut fsm, options).await? {
            return Ok(stats);
        }
        app.tick();
        send_frame_barrier(app, handle, local_team, &mut frame_counter).await?;

        if let Some(sink) = sink.as_deref_mut() {
            if !sink.frame(app) {
                tracing::info!("окно закрыто — завершаем сессию");
                return Ok(stats);
            }
        }

        // Приказы, собранные вводом в этом кадре (38 — движение, 08 — атака):
        // локальное применение + multicast в сессию (M4.3).
        flush_hud_orders(app, handle).await?;

        // 06 — ровно один раз за ход, когда локальная команда закончила.
        if app.hud_mut().take_end_turn_request() {
            tracing::info!(
                "HUD: End Turn нажат — закрываем ход команды {} (06 + 52)",
                local_team.0
            );
        }
        if !fsm.local_finished_sent() {
            let finish = Command::FinishedTurn(Packet06FinishedTurn { team: local_team });
            app.apply(finish.clone());
            handle
                .send_untracked(Packet::Command(finish))
                .await
                .context("отправка пакета 06")?;
            fsm.mark_local_finished();
        }

        // END (52).
        let packets = fsm.end(app.state_mut());
        send_barrier(handle, &packets).await?;
        if wait_barrier(
            app,
            handle,
            &mut stats,
            &mut fsm,
            &mut frame_counter,
            options,
            BarrierPhase::End,
            packets,
        )
        .await?
            == WaitOutcome::Stopped
        {
            return Ok(stats);
        }
        handle.clear_barriers().await?;
        tracing::debug!("барьер 52 (конец хода) пройден");

        // SYNC (54): авторитетное состояние своей команды.
        let packets = fsm.sync(app.state());
        send_barrier(handle, &packets).await?;
        if wait_barrier(
            app,
            handle,
            &mut stats,
            &mut fsm,
            &mut frame_counter,
            options,
            BarrierPhase::Sync,
            packets,
        )
        .await?
            == WaitOutcome::Stopped
        {
            return Ok(stats);
        }
        handle.clear_barriers().await?;
        tracing::debug!("барьер 54 (состояние команды) пройден");

        // CRC (45): свой CRC после 54-коррекции, затем сверка с пирами.
        let packets = fsm.crc(app.state_mut());
        send_barrier(handle, &packets).await?;
        if wait_barrier(
            app,
            handle,
            &mut stats,
            &mut fsm,
            &mut frame_counter,
            options,
            BarrierPhase::Crc,
            packets,
        )
        .await?
            == WaitOutcome::Stopped
        {
            return Ok(stats);
        }
        handle.clear_barriers().await?;

        if fsm.crc_matches(app.state()) {
            tracing::debug!("барьер 45 (CRC 0x{:04X}) пройден", fsm.crc_checksum);
        } else {
            let peers: Vec<String> = fsm
                .remote_teams(app.state())
                .map(|team| format!("{}:0x{:04X}", team, app.state().team_data_crc16[team]))
                .collect();
            tracing::warn!(
                "CRC 45: свой 0x{:04X}, у пиров [{}] — десинк (ждём 51/desync-отчёт)",
                fsm.crc_checksum,
                peers.join(", ")
            );
        }

        // finish_turn: 52 закрыт, все пиры переходят на следующий ход.
        if app.state().game_state != GameStateType::EndTurn {
            app.state_mut().game_state = GameStateType::EndTurn;
        }
        let game_over = app.state_mut().finish_turn();
        fsm.on_turn_finished();
        stats.turns_finished = stats.turns_finished.saturating_add(1);
        tracing::info!(
            "ход закрыт: turn_counter={} (всего {})",
            app.state().turn_counter,
            stats.turns_finished
        );

        if game_over {
            tracing::info!("партия завершена (check_end_game_conditions)");
            break;
        }

        if options.max_turns > 0 && stats.turns_finished >= options.max_turns {
            tracing::info!("достигнут лимит ходов (--turns {})", options.max_turns);
            break;
        }

        // 00 следующего хода — только в SimultaneousMoves.
        if fsm.simultaneous {
            let packets = fsm.begin(app.state_mut());
            send_barrier(handle, &packets).await?;
            if wait_barrier(
                app,
                handle,
                &mut stats,
                &mut fsm,
                &mut frame_counter,
                options,
                BarrierPhase::Begin,
                packets,
            )
            .await?
                == WaitOutcome::Stopped
            {
                break;
            }
            handle.clear_barriers().await?;
            tracing::debug!("барьер 00 следующего хода пройден");
        }

        tokio::time::sleep(options.tick_interval).await;
    }

    Ok(stats)
}

/// Обрабатывает одно событие сессии; `false` — сессия завершена.
fn handle_session_event(
    app: &mut App,
    event: SessionEvent,
    stats: &mut GameLoopStats,
    fsm: &mut TurnBarrierFsm,
) -> Result<bool> {
    match event {
        SessionEvent::Packet(Packet::Command(command)) => {
            // Пакет 13 — `Remote_RngSeed`. Мир уже сгенерирован с этим seed из
            // настроек 30 (`initialize_game`), а `GameManager_PopulateMapWithResources`
            // в C++ пере-сидирует ГПСЧ и продолжает поток. Повторное
            // `Randomizer_SetSeed` посреди партии сбросило бы поток и разошлось
            // бы с симуляцией сервера.
            if matches!(command, Command::Seed(_)) {
                tracing::debug!("13: seed уже применён при инициализации мира");
                return Ok(true);
            }

            // 53 — не только применение итогов баз, но и факт для барьера.
            if let Command::ComplexSummary(summary) = &command {
                fsm.note_complex(summary.team, summary.turn);
            }
            // 18 — чат: `apply_command` пишет в `GameState::chat_log`, но
            // игрок видит журнал HUD, поэтому дублируем строку туда же в
            // формате C++ (`Remote_ReceiveNetPacket_18`: `<team>: <text>`).
            if let Command::Chat(chat) = &command {
                let line = crate::hud::chat_line(chat.sender, &chat.message);
                app.hud_mut().push_log(line);
            }
            // 00 — команда ВСЕГДА из поля `team` payload'а, а не из
            // ENet-отправителя: релейная копия приходит от сервера, у которого
            // своей команды нет (`docs/barrier-flow.md` §1, `remote.cpp:2313`).
            if let Command::TurnBarrier(barrier) = &command {
                tracing::debug!(
                    "00: команда {} счётчик {} (team из payload)",
                    barrier.team.0,
                    barrier.counter
                );
            }
            app.apply(command);
        }
        SessionEvent::Packet(Packet::StateSync(sync)) => {
            // Порт `Remote_ReceiveNetPacket_54`: применяем авторитетное
            // состояние команды, иначе миры расходятся и CRC-барьер честно
            // покажет десинк.
            let packet = sync.to_core();
            let outcome = apply_state_sync(app.state_mut(), &packet);
            tracing::debug!(
                "54: команда {} индекс {} ({} юнитов, last={}) → {:?}",
                sync.team.0,
                sync.sync_index,
                sync.units.len(),
                sync.is_last,
                outcome
            );
        }
        SessionEvent::Packet(Packet::Desync { team, param }) => {
            tracing::warn!("сервер сообщил о десинке: команда {} param {}", team.0, param);
        }
        SessionEvent::Packet(Packet::Lobby(lobby)) => {
            tracing::debug!("игровой канал: лобби {lobby:?}");
        }
        SessionEvent::Packet(packet) => {
            tracing::debug!("игровой канал: {packet:?}");
        }
        SessionEvent::HandshakeOk => {}
        SessionEvent::BarrierDue { attempt, .. } => {
            tracing::debug!("барьер сессии: повтор #{attempt}");
        }
        SessionEvent::BarrierTimeout { .. } => {
            stats.barrier_timeouts = stats.barrier_timeouts.saturating_add(1);
            tracing::warn!("барьер сессии: таймаут пиров");
        }
        SessionEvent::HandshakeRejected {
            reason,
            reason_text,
        } => {
            return Err(anyhow!("хендшейк отклонён: {reason:?} ({reason_text})"));
        }
        SessionEvent::Disconnected => {
            tracing::info!("сервер отключил клиента");
            return Ok(false);
        }
        SessionEvent::Malformed { error } => {
            tracing::warn!("битый пакет: {error}");
        }
        SessionEvent::Error(error) => {
            return Err(error).context("ошибка сессии");
        }
    }

    Ok(true)
}

/// Диагностический текст для `TicketKind` (короткий, для логов).
trait TicketKindShort {
    fn kind_short(&self) -> String;
}

impl TicketKindShort for max_net::Ticket {
    fn kind_short(&self) -> String {
        match self.kind {
            max_net::TicketKind::Signed => "signed".to_string(),
            max_net::TicketKind::Legacy => "legacy".to_string(),
            max_net::TicketKind::Unspecified => "unspecified".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use max_core::enums::TeamClan;
    use max_core::input::{Packet18Chat, Packet45CrcBarrier};
    use max_core::team::TeamUnits;

    /// Синтетический мир: local RED (Player), пиры GREEN/BLUE (Remote).
    fn synthetic_state(local: TeamId, remotes: &[TeamId]) -> GameState {
        let mut state = GameState::new();
        for team in 0..PLAYER_TEAM_COUNT {
            state.teams[team].team_type = TeamKind::None;
            state.team_units[team] = TeamUnits::with_clan(TeamClan::Random);
        }
        state.teams[local.0 as usize].team_type = TeamKind::Player;
        for team in remotes {
            state.teams[team.0 as usize].team_type = TeamKind::Remote;
        }
        state.player_team = local;
        state
    }

    fn barrier_counter(packet: &Packet) -> u8 {
        match packet {
            Packet::Command(Command::TurnBarrier(barrier)) => barrier.counter,
            Packet::Command(Command::EndTurnBarrier(barrier)) => barrier.counter,
            Packet::Command(Command::CrcBarrier(barrier)) => barrier.next_turn_index,
            other => panic!("not a counter barrier: {other:?}"),
        }
    }

    #[test]
    fn counters_start_at_one_and_are_pre_incremented_per_team() {
        let local = TeamId::RED;
        let mut state = synthetic_state(local, &[TeamId::GREEN, TeamId::BLUE]);
        let mut fsm = TurnBarrierFsm::new(local, PlayMode::TurnBased);

        let begin = fsm.begin(&mut state);
        assert_eq!(begin.len(), 1);
        assert_eq!(barrier_counter(&begin[0]), 1, "первый 00 = 1");
        assert_eq!(state.frame_sync_counter[local.0 as usize], 1);
        // Собственный счётчик ещё не согласован с пирами.
        assert!(!fsm.begin_reached(&state));
        state.frame_sync_counter[TeamId::GREEN.0 as usize] = 1;
        assert!(!fsm.begin_reached(&state), "нужны все пиры");
        state.frame_sync_counter[TeamId::BLUE.0 as usize] = 1;
        assert!(fsm.begin_reached(&state));

        let end = fsm.end(&mut state);
        assert_eq!(barrier_counter(&end[0]), 1, "первый 52 = 1");
        assert_eq!(state.turn_indices[local.0 as usize], 1);
        assert!(!fsm.end_reached(&state));
        state.turn_indices[TeamId::GREEN.0 as usize] = 1;
        state.turn_indices[TeamId::BLUE.0 as usize] = 1;
        assert!(fsm.end_reached(&state));

        let sync = fsm.sync(&state);
        assert!(!sync.is_empty());
        match &sync[0] {
            Packet::StateSync(packet) => {
                assert_eq!(packet.sync_index, 1, "первый 54 = 1");
                assert_eq!(packet.team, local);
                assert!(packet.is_last, "у пустой команды один last-чанк");
            }
            other => panic!("expected 54, got {other:?}"),
        }
        assert!(!fsm.sync_reached(&state));
        state.state_sync_indices[TeamId::GREEN.0 as usize] = 1;
        state.state_sync_indices[TeamId::BLUE.0 as usize] = 1;
        assert!(fsm.sync_reached(&state));

        let crc = fsm.crc(&mut state);
        assert_eq!(barrier_counter(&crc[0]), 1, "первый 45 = 1");
        assert_eq!(state.next_turn_indices[local.0 as usize], 1);
        let own_crc = match &crc[0] {
            Packet::Command(Command::CrcBarrier(barrier)) => barrier.crc16,
            other => panic!("expected 45, got {other:?}"),
        };
        assert_eq!(own_crc, max_core::crc::state_checksum(&state));
        assert!(!fsm.crc_reached(&state));
        state.next_turn_indices[TeamId::GREEN.0 as usize] = 1;
        state.next_turn_indices[TeamId::BLUE.0 as usize] = 1;
        assert!(fsm.crc_reached(&state));
        assert!(!fsm.crc_matches(&state), "CRC пиров ещё не пришли");
        state.team_data_crc16[TeamId::GREEN.0 as usize] = own_crc;
        state.team_data_crc16[TeamId::BLUE.0 as usize] = own_crc;
        assert!(fsm.crc_matches(&state));
    }

    #[test]
    fn fsm_walks_the_full_turn_sequence() {
        let local = TeamId::RED;
        let mut state = synthetic_state(local, &[TeamId::GREEN]);
        let mut fsm = TurnBarrierFsm::new(local, PlayMode::TurnBased);

        // 00
        let begin = fsm.begin(&mut state);
        assert_eq!(fsm.phase, BarrierPhase::Begin);
        assert_eq!(barrier_counter(&begin[0]), 1);
        state.frame_sync_counter[TeamId::GREEN.0 as usize] = 1;
        assert!(fsm.reached(&state));

        // 53
        let play = fsm.complex(&state);
        assert_eq!(fsm.phase, BarrierPhase::Play);
        assert!(matches!(
            &play[0],
            Packet::Command(Command::ComplexSummary(summary)) if summary.turn == 1
        ));
        fsm.note_complex(TeamId::GREEN, 1);
        assert!(fsm.reached(&state));

        // 06 — локальный конец хода (ровно один раз).
        fsm.mark_local_finished();
        assert!(fsm.local_finished_sent());

        // 52
        let end = fsm.end(&mut state);
        assert_eq!(fsm.phase, BarrierPhase::End);
        assert_eq!(barrier_counter(&end[0]), 1);
        state.turn_indices[TeamId::GREEN.0 as usize] = 1;
        assert!(fsm.reached(&state));

        // 54
        let sync = fsm.sync(&state);
        assert_eq!(fsm.phase, BarrierPhase::Sync);
        assert!(matches!(&sync[0], Packet::StateSync(packet) if packet.sync_index == 1));
        state.state_sync_indices[TeamId::GREEN.0 as usize] = 1;
        assert!(fsm.reached(&state));

        // 45
        let crc = fsm.crc(&mut state);
        assert_eq!(fsm.phase, BarrierPhase::Crc);
        assert_eq!(barrier_counter(&crc[0]), 1);
        state.next_turn_indices[TeamId::GREEN.0 as usize] = 1;
        state.team_data_crc16[TeamId::GREEN.0 as usize] = fsm.crc_checksum;
        assert!(fsm.reached(&state));
        assert!(fsm.crc_matches(&state));

        // finish_turn → следующий ход начинается с чистого 06.
        fsm.on_turn_finished();
        assert!(!fsm.local_finished_sent());
    }

    #[test]
    fn same_kind_barriers_use_separate_counters() {
        let local = TeamId::RED;
        let mut state = synthetic_state(local, &[TeamId::GREEN]);
        let mut fsm = TurnBarrierFsm::new(local, PlayMode::TurnBased);

        fsm.begin(&mut state);
        fsm.end(&mut state);
        fsm.sync(&state);
        fsm.crc(&mut state);
        assert_eq!(fsm.begin_counter, 1);
        assert_eq!(fsm.end_counter, 1);
        assert_eq!(fsm.sync_index, 1);
        assert_eq!(fsm.crc_index, 1);

        // Следующий ход: у каждого счётчика свой pre-increment (2).
        fsm.on_turn_finished();
        let begin = fsm.begin(&mut state);
        assert_eq!(barrier_counter(&begin[0]), 2);
        let end = fsm.end(&mut state);
        assert_eq!(barrier_counter(&end[0]), 2);
        let sync = fsm.sync(&state);
        assert!(matches!(
            &sync[0],
            Packet::StateSync(packet) if packet.sync_index == 2
        ));
        let crc = fsm.crc(&mut state);
        assert_eq!(barrier_counter(&crc[0]), 2);

        // 52 и 45 — разные массивы C++, а не общий счётчик.
        assert_eq!(state.turn_indices[local.0 as usize], 2);
        assert_eq!(state.next_turn_indices[local.0 as usize], 2);
        assert_eq!(state.frame_sync_counter[local.0 as usize], 2);
    }

    #[test]
    fn complex_barrier_needs_53_from_every_peer_for_the_current_turn() {
        let local = TeamId::RED;
        let mut state = synthetic_state(local, &[TeamId::GREEN, TeamId::BLUE]);
        // Rust turn_counter = 0 → C++ GameManager_TurnCounter = 1.
        let mut fsm = TurnBarrierFsm::new(local, PlayMode::TurnBased);
        let packet = fsm.complex(&state);
        assert!(matches!(
            &packet[0],
            Packet::Command(Command::ComplexSummary(summary)) if summary.turn == 1
        ));
        assert!(!fsm.complex_reached(&state));

        fsm.note_complex(TeamId::GREEN, 1);
        assert!(!fsm.complex_reached(&state), "BLUE ещё не прислал 53");
        fsm.note_complex(TeamId::BLUE, 1);
        assert!(fsm.complex_reached(&state));

        // 53 за прошлый ход не закрывает новый.
        state.turn_counter = 1;
        let packet = fsm.complex(&state);
        assert!(matches!(
            &packet[0],
            Packet::Command(Command::ComplexSummary(summary)) if summary.turn == 2
        ));
        assert!(!fsm.complex_reached(&state));
        fsm.note_complex(TeamId::GREEN, 2);
        fsm.note_complex(TeamId::BLUE, 2);
        assert!(fsm.complex_reached(&state));
    }

    #[test]
    fn single_player_barriers_pass_immediately() {
        let local = TeamId::RED;
        let mut state = synthetic_state(local, &[]);
        let mut fsm = TurnBarrierFsm::new(local, PlayMode::TurnBased);

        let packets = fsm.begin(&mut state);
        assert!(!packets.is_empty());
        assert!(fsm.reached(&state), "00 без пиров");

        let packets = fsm.complex(&state);
        assert!(!packets.is_empty());
        assert!(fsm.reached(&state), "53 без пиров");

        let packets = fsm.end(&mut state);
        assert!(!packets.is_empty());
        assert!(fsm.reached(&state), "52 без пиров");

        let packets = fsm.sync(&state);
        assert!(!packets.is_empty());
        assert!(fsm.reached(&state), "54 без пиров");

        let packets = fsm.crc(&mut state);
        assert!(!packets.is_empty());
        assert!(fsm.reached(&state), "45 без пиров");
        assert!(fsm.crc_matches(&state));
    }

    #[test]
    fn turn_based_runs_begin_once_simultaneous_every_turn() {
        let local = TeamId::RED;

        let turn_based = TurnBarrierFsm::new(local, PlayMode::TurnBased);
        assert!(!turn_based.simultaneous);

        let simultaneous = TurnBarrierFsm::new(local, PlayMode::SimultaneousMoves);
        assert!(simultaneous.simultaneous);
    }

    #[test]
    fn local_06_is_sent_once_per_turn_and_reset_on_finish() {
        let local = TeamId::RED;
        let mut fsm = TurnBarrierFsm::new(local, PlayMode::TurnBased);

        assert!(!fsm.local_finished_sent());
        fsm.mark_local_finished();
        assert!(fsm.local_finished_sent());

        // Новый ход сбрасывает флаг: 06 отправится снова.
        fsm.on_turn_finished();
        assert!(!fsm.local_finished_sent(), "новый ход — новый 06");
    }

    #[test]
    fn barrier_pending_repeats_every_three_seconds_and_reports_timeout_once() {
        let start = Instant::now();
        let packet = Packet::Command(Command::CrcBarrier(Packet45CrcBarrier {
            team: TeamId::RED,
            next_turn_index: 1,
            crc16: 0,
        }));
        let mut pending = BarrierPending::new(vec![packet], start);

        assert!(pending.poll(start).is_none());
        assert!(matches!(
            pending.poll(start + Duration::from_secs(3)),
            Some(BarrierPoll::Retransmit)
        ));
        assert_eq!(pending.attempts, 1);
        assert!(matches!(
            pending.poll(start + Duration::from_secs(6)),
            Some(BarrierPoll::Retransmit)
        ));
        assert_eq!(pending.attempts, 2);

        // Таймаут логируется один раз, но запись не исчезает — барьер
        // продолжает повторяться, как C++-пинг.
        assert!(matches!(
            pending.poll(start + Duration::from_secs(30)),
            Some(BarrierPoll::TimedOut)
        ));
        assert!(matches!(
            pending.poll(start + Duration::from_secs(31)),
            Some(BarrierPoll::Retransmit)
        ));
        assert!(pending.poll(start + Duration::from_secs(32)).is_none());
    }

    #[test]
    fn team_kinds_come_from_welcome_team_nodes() {
        let kinds = team_kinds_from_nodes(11, &[11, 0, 12, 0]);
        assert_eq!(kinds[0], TeamKind::Player, "своя нода → Player");
        assert_eq!(kinds[1], TeamKind::None, "слот 0 не занят");
        assert_eq!(kinds[2], TeamKind::Remote, "чужая нода → Remote");
        assert_eq!(kinds[3], TeamKind::None);
    }

    #[test]
    fn team_kinds_ignore_nodes_at_or_below_four() {
        // Ноды 0..=4 — не клиенты (`Remote_SetupPlayers`: `> 4`).
        assert_eq!(
            team_kinds_from_nodes(1, &[1, 2, 3, 4]),
            [TeamKind::None; PLAYER_TEAM_COUNT]
        );
    }

    #[test]
    fn planet_comes_from_ini_world_index_or_world_name() {
        let mut settings = test_settings();
        settings.ini_world_index = 12;
        assert_eq!(planet_from_settings(&settings), Some(Planet::NewLuzon));

        settings.ini_world_index = 200;
        settings.world_name = max_net::WireName::from("DESERT_6");
        assert_eq!(planet_from_settings(&settings), Some(Planet::Bottleneck));

        settings.world_name = max_net::WireName::from("MULTI");
        assert_eq!(planet_from_settings(&settings), None, "неизвестное имя → None");
    }

    /// Релейная копия приходит от сервера, у которого нет команды: событие
    /// сессии несёт только payload. Кадр проходит через настоящий wire-кодек.
    fn relayed(packet: &Packet) -> SessionEvent {
        let frame = packet.encode().expect("encode barrier");
        SessionEvent::Packet(Packet::decode(&frame).expect("decode relayed frame"))
    }

    /// Пакет 00 от сервера с `team = 1` учитывается как команда 1.
    #[test]
    fn relayed_00_from_the_server_counts_for_the_payload_team() {
        let local = TeamId::RED;
        let mut app = App::new(AppConfig::default());
        *app.state_mut() = synthetic_state(local, &[TeamId::GREEN]);
        let mut fsm = TurnBarrierFsm::new(local, PlayMode::SimultaneousMoves);
        let mut stats = GameLoopStats::default();

        let packets = fsm.begin(app.state_mut());
        assert_eq!(barrier_counter(&packets[0]), 1, "счётчики с 1");
        assert_eq!(app.state().frame_sync_counter[local.0 as usize], 1);
        assert!(!fsm.reached(app.state()), "пир ещё не ответил");

        let relayed_00 = Packet::Command(Command::TurnBarrier(Packet00TurnBarrier {
            team: TeamId::GREEN,
            counter: 1,
        }));
        handle_session_event(&mut app, relayed(&relayed_00), &mut stats, &mut fsm)
            .expect("релейный 00");

        assert_eq!(
            app.state().frame_sync_counter[TeamId::GREEN.0 as usize], 1,
            "team берётся из payload, а не из ENet-отправителя (сервер)"
        );
        assert!(fsm.reached(app.state()), "00-барьер проходит по team=1");
    }

    /// Входящий 18 применяется (`GameState::chat_log`) и попадает в журнал
    /// HUD в формате C++ (`Remote_ReceiveNetPacket_18`).
    #[test]
    fn incoming_18_chat_goes_to_the_simulation_and_hud_logs() {
        let local = TeamId::RED;
        let mut app = App::new(AppConfig::default());
        *app.state_mut() = synthetic_state(local, &[TeamId::GREEN]);
        let mut fsm = TurnBarrierFsm::new(local, PlayMode::TurnBased);
        let mut stats = GameLoopStats::default();

        let incoming = Packet::Command(Command::Chat(Packet18Chat {
            sender: TeamId::GREEN,
            message: "hello".to_string(),
        }));

        handle_session_event(&mut app, relayed(&incoming), &mut stats, &mut fsm)
            .expect("входящий 18");

        assert_eq!(
            app.state().chat_log,
            vec![(TeamId::GREEN, "hello".to_string())]
        );
        assert_eq!(
            app.hud().log.lines().last().map(String::as_str),
            Some("GREEN: hello")
        );
        assert_eq!(app.applied_commands(), 1);
    }

    /// Два пира, релейные пакеты приходят от сервера — барьер 00 сходится.
    #[test]
    fn two_peers_pass_the_relayed_begin_barrier() {
        let mut apps = [
            App::new(AppConfig::default()),
            App::new(AppConfig::default()),
        ];
        *apps[0].state_mut() = synthetic_state(TeamId::RED, &[TeamId::GREEN]);
        *apps[1].state_mut() = synthetic_state(TeamId::GREEN, &[TeamId::RED]);

        let mut fsms = [
            TurnBarrierFsm::new(TeamId::RED, PlayMode::SimultaneousMoves),
            TurnBarrierFsm::new(TeamId::GREEN, PlayMode::SimultaneousMoves),
        ];
        let mut stats = GameLoopStats::default();

        let red = fsms[0].begin(apps[0].state_mut());
        let green = fsms[1].begin(apps[1].state_mut());
        assert_eq!(barrier_counter(&red[0]), 1);
        assert_eq!(barrier_counter(&green[0]), 1);

        // Сервер релеит 00(RED) пиру GREEN и 00(GREEN) пиру RED.
        handle_session_event(&mut apps[1], relayed(&red[0]), &mut stats, &mut fsms[1])
            .expect("00 RED → GREEN");
        handle_session_event(&mut apps[0], relayed(&green[0]), &mut stats, &mut fsms[0])
            .expect("00 GREEN → RED");

        assert!(
            fsms[0].reached(apps[0].state()),
            "RED ждал FrameSyncCounter[GREEN] == 1"
        );
        assert!(
            fsms[1].reached(apps[1].state()),
            "GREEN ждал FrameSyncCounter[RED] == 1"
        );
        assert_eq!(apps[0].state().frame_sync_counter[TeamId::GREEN.0 as usize], 1);
        assert_eq!(apps[1].state().frame_sync_counter[TeamId::RED.0 as usize], 1);
    }

    fn test_settings() -> LobbyGameSettings {
        LobbyGameSettings {
            mission_category: max_core::victory::MissionCategory::Multiplayer,
            multi_scenario_id: Some(1),
            mission_hashes: Vec::new(),
            world_name: max_net::WireName::from("SNOW_1"),
            ini_world_index: 0,
            ini_play_mode: 0,
            ini_timer: 3,
            ini_endturn: 0,
            ini_opponent: 1,
            ini_start_gold: 200,
            ini_raw_resource: 1,
            ini_fuel_resource: 1,
            ini_gold_resource: 1,
            ini_alien_derelicts: 0,
            ini_victory_type: 0,
            ini_victory_limit: 50,
            is_map_changed: false,
            is_multi_scenario: false,
            default_team_names: [max_net::WireName::EMPTY; 4],
            rng_seed: 0x0102_0304,
        }
    }
}
