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
//! 4. Игровой цикл ([`run_network_with_sink`]): события сессии → [`Command`] →
//!    [`App::apply`], тик симуляции, барьеры `00/01/45/52` (§8.6) →
//!    [`max_core::game::GameState::finish_turn`].
//!
//! Сервер-арбитр не эхо-шлёт барьеры клиента, поэтому:
//!
//! * [`SessionHandle`] сам повторяет `00/45/52` каждые 3 с и сообщает
//!   `BarrierDue`/`BarrierTimeout`;
//! * `max_app` дополнительно держит [`BarrierTracker`] для логики «конец хода»
//!   (ждёт, пока все играющие команды пришлют одинаковый счётчик);
//! * свой пакет 52 приложение применяет локально (как `Remote_Synchronize`),
//!   иначе собственный счётчик не попадёт в [`GameState::end_turn_barrier`].

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use max_core::enums::GameStateType;
use max_core::input::{
    Command, Packet00TurnBarrier, Packet01FrameBarrier, Packet06FinishedTurn,
    Packet52EndTurnBarrier,
};
use max_core::team::{TeamId, TeamKind};
use max_net::handshake::DataHash;
use max_net::{
    BarrierAction, BarrierTiming, BarrierTracker, GameJoined, HorizonClient, LobbyGameSettings,
    LobbyPacket, Packet, SessionConfig, SessionError, SessionEvent, SessionHandle, CLIENT_NAME,
    HORIZON_PROTOCOL_VERSION,
};
use tokio::time::Instant;

use crate::{App, AppConfig, AppDigest};

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
pub trait FrameSink {
    /// Показать кадр текущего состояния; `false` — выйти из цикла.
    fn frame(&mut self, app: &App) -> bool;
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
                    info.settings = Some(welcome.settings.clone());

                    let team_slot = config.team_slot.min(3);
                    let team_name = format!("{}#{team_slot}", config.player_name);

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
                        "лобби: 30 player_node={} host_node={} world={} seed={} → 36/35 (слот {team_slot}, READY)",
                        info.player_node,
                        info.host_node,
                        welcome.settings.world_name,
                        welcome.settings.rng_seed
                    );
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
/// `sink` получает кадр после каждого тика (SDL-окно в бинарнике). Если
/// [`FrameSink::frame`] вернул `false`, цикл завершается и сессия закрывается
/// штатно.
pub async fn run_network_with_sink(
    app: &mut App,
    options: &NetworkOptions,
    mut sink: Option<&mut dyn FrameSink>,
) -> Result<NetworkReport> {
    let config = app.config().clone();
    let data_hash =
        DataHash::from_files(&config.max_res, &config.patches_res).with_context(|| {
            format!(
                "не удалось посчитать SHA256({} + {})",
                config.max_res.display(),
                config.patches_res.display()
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
    let local_team = TeamId(config.team_slot.min(3));

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

#[derive(Debug, Default)]
struct GameLoopStats {
    turns_finished: u32,
    barrier_timeouts: u32,
}

/// Игровой цикл lockstep-клиента.
async fn run_game_loop(
    app: &mut App,
    handle: &mut SessionHandle,
    local_team: TeamId,
    options: &NetworkOptions,
    sink: &mut Option<&mut dyn FrameSink>,
) -> Result<GameLoopStats> {
    let mut stats = GameLoopStats::default();
    let mut barrier = BarrierTracker::new(BarrierTiming::default());
    let mut turn_barrier: Option<(i32, u8)> = None;
    let mut frame_counter: u8 = 0;
    let mut turn_counter: u8 = 0;
    let mut stopping = false;

    // Барьер начала первого хода (00).
    let start = Command::TurnBarrier(Packet00TurnBarrier {
        team: local_team,
        counter: turn_counter,
    });
    handle
        .send(Packet::Command(start.clone()))
        .await
        .context("отправка барьера 00")?;
    app.apply(start);

    while !stopping {
        // 1. Все накопившиеся события сессии (порядок = порядок команд).
        loop {
            match handle.next_event_timeout(options.event_wait).await {
                Ok(Some(event)) => {
                    if !handle_session_event(app, event, &mut stats)? {
                        stopping = true;
                        break;
                    }
                }
                Ok(None) => {
                    stopping = true;
                    break;
                }
                Err(SessionError::Timeout { .. }) => break,
                Err(error) => return Err(error).context("ожидание события сессии"),
            }
        }

        if stopping {
            break;
        }

        // 2. Тик симуляции.
        app.tick();

        // 3. Покадровый барьер 01 (real-time pacing, §8.6).
        frame_counter = (frame_counter + 1) & 0x3F;
        let frame_barrier = Command::FrameBarrier(Packet01FrameBarrier {
            team: local_team,
            counter: frame_counter,
        });
        handle
            .send(Packet::Command(frame_barrier.clone()))
            .await
            .context("отправка барьера 01")?;
        app.apply(frame_barrier);

        // 4. Конец хода: 06 (локальная команда закончила) и 52 (барьер счётчика).
        let ready = app.state().game_state == GameStateType::EndTurn
            || app.state().are_teams_finished_turn();

        if ready {
            if !app.state().teams[local_team.0 as usize].finished_turn
                && app.state().teams[local_team.0 as usize].team_type == TeamKind::Player
            {
                let finish = Command::FinishedTurn(Packet06FinishedTurn { team: local_team });
                handle
                    .send(Packet::Command(finish.clone()))
                    .await
                    .context("отправка пакета 06")?;
                app.apply(finish);
            }

            let current_turn = app.state().turn_counter;

            if turn_barrier.map(|(turn, _)| turn) != Some(current_turn) {
                turn_counter = turn_counter.wrapping_add(1);
                let command = Command::EndTurnBarrier(Packet52EndTurnBarrier {
                    team: local_team,
                    counter: turn_counter,
                });
                // Свой счётчик применяем локально: сервер не эхо-шлёт 52.
                app.apply(command.clone());
                handle
                    .send(Packet::Command(command.clone()))
                    .await
                    .context("отправка барьера 52")?;
                barrier.track(Packet::Command(command), Instant::now());
                turn_barrier = Some((current_turn, turn_counter));
                tracing::debug!("ход {current_turn}: отправлен барьер 52 #{turn_counter}");
            }

            let agreed = app.state().end_turn_barrier() == turn_barrier.map(|(_, counter)| counter);

            if agreed {
                // `finish_turn` требует фазу EndTurn (в TurnBased её ставит 06).
                if app.state().game_state != GameStateType::EndTurn {
                    app.state_mut().game_state = GameStateType::EndTurn;
                }

                barrier.clear();
                handle
                    .clear_barriers()
                    .await
                    .context("сброс ретрансляции барьеров")?;
                turn_barrier = None;

                let game_over = app.state_mut().finish_turn();

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

                // Барьер начала следующего хода.
                let start = Command::TurnBarrier(Packet00TurnBarrier {
                    team: local_team,
                    counter: turn_counter,
                });
                handle
                    .send(Packet::Command(start.clone()))
                    .await
                    .context("отправка барьера 00")?;
                app.apply(start);
            }
        }

        // 5. Собственный трекер барьеров: повтор/таймаут.
        for action in barrier.poll(Instant::now()) {
            match action {
                BarrierAction::Retransmit { packet, attempt } => {
                    tracing::debug!("барьер 52: повтор #{attempt}");
                    handle.send(packet).await.context("повтор барьера 52")?;
                }
                BarrierAction::TimedOut { .. } => {
                    stats.barrier_timeouts = stats.barrier_timeouts.saturating_add(1);
                    tracing::warn!(
                        "барьер 52 не дождался пиров за {:?}",
                        BarrierTiming::default().timeout
                    );
                }
            }
        }

        // 6. Кадр (SDL-окно) и пауза тика.
        if let Some(sink) = sink.as_deref_mut() {
            if !sink.frame(app) {
                tracing::info!("окно закрыто — завершаем сессию");
                break;
            }
        }

        if options.max_turns > 0 && stats.turns_finished >= options.max_turns {
            tracing::info!("достигнут лимит ходов (--turns {})", options.max_turns);
            break;
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
) -> Result<bool> {
    match event {
        SessionEvent::Packet(Packet::Command(command)) => {
            app.apply(command);
        }
        SessionEvent::Packet(Packet::StateSync(sync)) => {
            tracing::info!("синхронизация состояния: индекс {}", sync.sync_index);
        }
        SessionEvent::Packet(Packet::Desync { .. }) => {
            tracing::warn!("сервер сообщил о десинке");
        }
        SessionEvent::Packet(Packet::Lobby(lobby)) => {
            tracing::debug!("игровой канал: лобби {lobby:?}");
        }
        SessionEvent::Packet(packet) => {
            tracing::debug!("игровой канал: {packet:?}");
        }
        SessionEvent::HandshakeOk => {}
        SessionEvent::BarrierDue { attempt, .. } => {
            tracing::debug!("барьер 00/45/52: повтор #{attempt} (сессия)");
        }
        SessionEvent::BarrierTimeout { .. } => {
            stats.barrier_timeouts = stats.barrier_timeouts.saturating_add(1);
            tracing::warn!("барьер 00/45/52: таймаут пиров (сессия)");
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
