//! Network mode: Horizon (gRPC) → ENet session → in-game lobby → game.
//!
//! The sequence matches `max-brh-docs/protocol.md` §2/§7/§8:
//!
//! 1. [`connect_horizon`] — `Hello` + `JoinGame`; the [`GameJoined`] response carries
//!    the `host`/`port`/`ticket` of the UDP session (`max-brh-docs/protocol-grpc.md` §4).
//! 2. [`SessionConfig::from_joined`] + [`SessionHandle::start`] — ENet connection
//!    and `TP 00`/`TP 03` (channel 0).
//! 3. In-game lobby (channel 1): `28 → 44 → 29 → 30 → 36 → 35 → 32 → 05 → 34`
//!    (§8.2). [`run_lobby`] performs this part.
//! 4. Game loop ([`run_network_with_sink`]): world/seed/team types from
//!    `LobbyGameSettings` (packet 30), then the turn FSM
//!    `00 → 53 → ticks → 06 → 52 → 54 → 45 → finish_turn`
//!    (`max-brh-docs/barrier-flow.md` §3) → [`max_core::game::GameState::finish_turn`].
//!
//! # Barriers
//!
//! The arbiter server does not echo the client's barriers: a barrier is complete when **each
//! remote team** has sent a counter equal to its own (`Remote_Wait*`,
//! `remote.cpp`). Therefore:
//!
//! * [`TurnBarrierFsm`] keeps separate counters 00/52/54/45, all
//!   pre-increment (the first report is **1**, like `++Remote_FrameSyncCounter` in C++);
//! * each barrier is actually awaited (`wait_barrier`) with a retry every 3 s and
//!   a 30 s timeout; after passing, the counters/tracker are cleared;
//! * retries go through [`SessionHandle::send_untracked`] so that the session
//!   tracker does not get a second record for each retransmission
//!   (`max-brh-docs/barrier-flow.md` §6.8);
//! * the incoming 54 is applied to [`max_core::game::GameState`]
//!   (`max_core::events::apply_state_sync`), after the 45 barrier the team CRCs
//!   are compared with its own (`Remote_TeamDataCrc16`).
//!
//! # Reconnect
//!
//! `SessionEvent::Disconnected` starts [`ReconnectPolicy`]: the client again
//! does an ENet connect + `TP 00` with the same ticket (with the timeout and number of
//! attempts from `--reconnect-attempts`/`--reconnect-timeout`), without a second lobby.
//! The local lockstep state is not reset: while the client was offline,
//! the peers/server waited for its barrier. After `TP 00` [`TurnBarrierFsm::resume_state_sync`]
//! publishes its team's 54 (units+economy, the last `sync_index`),
//! the peers' 54 correct the remote teams, then 52/54/45 and `finish_turn`
//! are replayed; `reconnect_pending_state=false`.
//!
//! Limits (`max-brh-docs/protocol.md` §9): there is no SNAP packet, 54 covers only
//! one team's units/economy, missed orders of other teams are not
//! replayed, C++ peers exclude the team after 30 s. Lobby reconnect is not
//! supported (the server is already not in `Remote_GameState=1`).
//!
//! # First-connection retry and `data_hash` (M-NET follow-ups)
//!
//! After `Create game` the headless session takes 10–15 s to come up, and the first
//! `ENet connect` fails (`connect timed out after 10s`). [`open_network_session`]
//! repeats `SessionHandle::start` per [`JoinRetryPolicy`] (backoff
//! 1,2,3,5,8,11 s, window ~30 s), without returning the player to the game list; progress
//! goes to [`FrameSink::join_status`] ("Session warming up — attempt N/M").
//!
//! `data_hash` is negotiated without `--handshake-patches`: [`handshake_data_hashes`]
//! gives the full hash (`MAX.RES` + `PATCHES.RES`), then the `MAX.RES`-only hash;
//! after `TP 03 reason=GameData` the client reconnects once with the second
//! variant (same ticket — Horizon `JoinGame` is not repeated). Any other
//! `TP 03` (`Version`, `SessionTicket`) does not start the fallback.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use max_core::enums::{GameStateType, Planet, PlayMode, VictoryType};
use max_core::events::apply_state_sync;
use max_core::game::{GameState, PLAYER_TEAM_COUNT};
use max_core::geo::Point;
use max_core::input::{
    Command, Packet00TurnBarrier, Packet01FrameBarrier, Packet06FinishedTurn,
    Packet52EndTurnBarrier,
};
use max_core::setup::{ResourceLevel, TeamStart};
use max_core::team::{TeamId, TeamKind};
use max_net::handshake::{DataHash, HandshakeError, RejectReason};
use max_net::session::{DEFAULT_BARRIER_REPEAT, DEFAULT_BARRIER_TIMEOUT};
use max_net::{
    complex_summary_for, crc_barrier_for, state_sync_for, GameJoined, HorizonClient,
    LobbyGameSettings, LobbyPacket, Packet, SessionConfig, SessionError, SessionEvent,
    SessionHandle, CLIENT_NAME, HORIZON_PROTOCOL_VERSION,
};
use tokio::time::Instant;

use crate::{App, AppConfig, AppDigest, NewGameConfig, WORLD_ASSETS};

/// How long to wait for one session event before ticking the simulation.
pub const DEFAULT_EVENT_WAIT: Duration = Duration::from_millis(2);
/// Pause between network loop ticks (the client must not spin the CPU at 100%).
pub const DEFAULT_TICK_INTERVAL: Duration = Duration::from_millis(20);
/// How long to wait for a complete lobby (28 → 34).
pub const DEFAULT_LOBBY_TIMEOUT: Duration = Duration::from_secs(30);

/// Network mode parameters (`--connect`).
#[derive(Debug, Clone)]
pub struct NetworkOptions {
    /// `game_id` from the Horizon lobby (`--join`).
    pub game_id: String,
    /// Game password (optional).
    pub password: Option<String>,
    /// How many turns to play; `0` — until a disconnect/stop.
    pub max_turns: u32,
    /// Lobby timeout.
    pub lobby_timeout: Duration,
    /// Pause between ticks.
    pub tick_interval: Duration,
    /// Session event wait per iteration.
    pub event_wait: Duration,
    /// Reconnect policy on ENet session disconnect.
    pub reconnect: ReconnectPolicy,
    /// Retry policy of the **first** connection (Create game → Join → start):
    /// the headless session warms up for 10–15 s, and the ENet connect fails in this window.
    pub join_retry: JoinRetryPolicy,
    /// How long to wait for player input in the PLAY phase before closing the turn.
    ///
    /// `Duration::ZERO` (default) — headless behavior: tick, frame and immediately
    /// `06 + 52`, the turn does not wait for a human (this is how `--connect`, live smoke and
    /// tests work). `--turn-wait N` enables the interactive window: with an SDL window the loop
    /// runs ticks/frames and keeps the turn open until **End Turn** is pressed or
    /// `N` seconds expire.
    pub turn_wait: Duration,
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
            reconnect: ReconnectPolicy::default(),
            join_retry: JoinRetryPolicy::default(),
            turn_wait: Duration::ZERO,
        }
    }
}

impl NetworkOptions {
    /// Parameters for the game `game_id` with default timeouts.
    pub fn new(game_id: impl Into<String>) -> Self {
        Self {
            game_id: game_id.into(),
            ..Self::default()
        }
    }
}

/// How many reconnect attempts to make by default.
pub const DEFAULT_RECONNECT_ATTEMPTS: u32 = 3;
/// Timeout of one attempt: ENet connect + `TP 00`.
pub const DEFAULT_RECONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Pause between failed attempts.
pub const DEFAULT_RECONNECT_BACKOFF: Duration = Duration::from_secs(1);

/// Reconnect policy after an ENet session disconnect.
///
/// `attempts = 0` disables automatic reconnect. The transport is
/// restored via an ENet connect + `TP 00`; the game state
/// continues from the local lockstep point and catches up via resume-54
/// (`max-brh-docs/protocol.md` §9). There is still no SNAP/full snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectPolicy {
    /// Maximum number of attempts (`0` — do not reconnect).
    pub attempts: u32,
    /// Timeout of one attempt (ENet connect + `TP 00`).
    pub timeout: Duration,
    /// Pause between failed attempts.
    pub backoff: Duration,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            attempts: DEFAULT_RECONNECT_ATTEMPTS,
            timeout: DEFAULT_RECONNECT_TIMEOUT,
            backoff: DEFAULT_RECONNECT_BACKOFF,
        }
    }
}

/// How many first-connection attempts to make by default.
///
/// This is **not** a reconnect: after `Create game` the headless session comes up in
/// 10–15 s, and `ENet connect` simply does not answer before that
/// (`connect timed out after 10s`). The retry makes Create → InSession
/// self-sufficient, without "create → wait → Join from the list".
pub const DEFAULT_JOIN_RETRY_ATTEMPTS: u32 = 6;
/// Timeout of one first-connection attempt (ENet connect + `TP 00`).
pub const DEFAULT_JOIN_RETRY_TIMEOUT: Duration = Duration::from_secs(4);
/// Total retry window: new attempts do not start after it.
pub const DEFAULT_JOIN_RETRY_BUDGET: Duration = Duration::from_secs(30);

/// Delays before retries: 1, 2, 3, 5, 8, 11 s (sum 30 s).
pub const JOIN_RETRY_BACKOFF: [Duration; 6] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(3),
    Duration::from_secs(5),
    Duration::from_secs(8),
    Duration::from_secs(11),
];

/// Retry policy of the first connection to the session (`Create game → Join → start`).
///
/// `attempts` — **total** attempts (`0` is treated as one attempt without
/// retries); `timeout` — the timeout of one attempt (`ENet connect + TP 00`);
/// `budget` — the window after which new attempts do not start (by default
/// 30 s). Delays between attempts — [`JOIN_RETRY_BACKOFF`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JoinRetryPolicy {
    /// Total connection attempts (`0` — one, without retries).
    pub attempts: u32,
    /// Timeout of one attempt (ENet connect + `TP 00`).
    pub timeout: Duration,
    /// Total retry window; `Duration::ZERO` — no limit.
    pub budget: Duration,
}

impl Default for JoinRetryPolicy {
    fn default() -> Self {
        Self {
            attempts: DEFAULT_JOIN_RETRY_ATTEMPTS,
            timeout: DEFAULT_JOIN_RETRY_TIMEOUT,
            budget: DEFAULT_JOIN_RETRY_BUDGET,
        }
    }
}

impl JoinRetryPolicy {
    /// Delay before the next attempt after `failures` failures.
    pub fn backoff_after(failures: u32) -> Duration {
        if failures == 0 {
            return Duration::ZERO;
        }
        let index = (failures - 1) as usize;
        JOIN_RETRY_BACKOFF
            .get(index)
            .copied()
            .unwrap_or_else(|| *JOIN_RETRY_BACKOFF.last().expect("непустая таблица задержек"))
    }

    /// Total attempts, where `0` = "one, without retries".
    pub fn total_attempts(self) -> u32 {
        self.attempts.max(1)
    }
}

/// First-connection progress — for the UI status line and the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinProgress {
    /// The next attempt `attempt` out of `total` (the session is warming up).
    Attempt {
        /// Attempt number (1-based).
        attempt: u32,
        /// Total attempts by the policy.
        total: u32,
    },
    /// The server rejected `data_hash` (`TP 03`, reason `GameData`) — the client
    /// reconnects with the fallback hash (`MAX.RES` only).
    FallbackDataHash {
        /// Hash variant number (1-based) currently being connected with.
        hash: u32,
        /// How many variants in total.
        hashes: u32,
    },
}

impl JoinProgress {
    /// Status line for the window/menu (English — like the whole original UI).
    pub fn status_text(self) -> String {
        match self {
            Self::Attempt { attempt, total } => {
                format!("Session warming up — attempt {attempt}/{total}")
            }
            Self::FallbackDataHash { hash, hashes } => format!(
                "Server expects the MAX.RES-only data_hash — reconnecting (hash {hash}/{hashes})"
            ),
        }
    }
}

/// Error of one connection attempt (Horizon JoinGame or ENet + `TP 00`).
#[derive(Debug, thiserror::Error)]
pub enum JoinAttemptError {
    /// Horizon `JoinGame` did not answer/answered with an error.
    #[error("Horizon JoinGame: {0}")]
    Horizon(String),
    /// ENet connect / `TP 00`.
    #[error(transparent)]
    Session(#[from] SessionError),
    /// External attempt timeout ([`JoinRetryPolicy::timeout`]).
    #[error("join timed out after {0:?}")]
    Timeout(Duration),
}

impl JoinAttemptError {
    /// Whether the attempt is worth retrying: timeout/transport — yes, `TP 03` — no.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Horizon(_) | Self::Timeout(_) => true,
            Self::Session(error) => match error {
                // Version/ticket/data and a broken TP 00 — a server response, retrying
                // will change nothing.
                SessionError::Handshake(_) | SessionError::Closed | SessionError::Wire(_) => false,
                // Connect/handshake timeout, disconnect, ENet, DNS — transport.
                SessionError::Timeout { .. }
                | SessionError::Disconnected { .. }
                | SessionError::Enet(_)
                | SessionError::Resolution { .. } => true,
            },
        }
    }

    /// `TP 03` with `reason = GameData` (the server expects a different `data_hash`).
    pub fn is_game_data_reject(&self) -> bool {
        matches!(
            self,
            Self::Session(SessionError::Handshake(HandshakeError::Rejected {
                reason: RejectReason::GameData,
                ..
            }))
        )
    }
}

/// Outcome of retrying one connection stage.
enum JoinRetryOutcome<T> {
    /// The connection succeeded on attempt `attempts`.
    Connected {
        /// Connector result.
        value: T,
        /// Number of the successful attempt.
        attempts: u32,
    },
    /// The attempts ran out (the errors were retryable).
    Exhausted {
        /// The last error.
        error: JoinAttemptError,
        /// How many attempts were made.
        attempts: u32,
    },
    /// An error that is pointless to retry (`TP 03` etc.).
    Rejected {
        /// Server error.
        error: JoinAttemptError,
        /// How many attempts were made.
        attempts: u32,
    },
}

/// Performs connection attempts with backoff and the total window.
///
/// The function does not know what is being connected: tests pass a mock connector,
/// production code — Horizon `JoinGame` and [`SessionHandle::start`]. Each attempt
/// is bounded by [`JoinRetryPolicy::timeout`], pauses — [`JOIN_RETRY_BACKOFF`],
/// new attempts do not start after [`JoinRetryPolicy::budget`].
async fn retry_join<T, F, Fut>(
    policy: JoinRetryPolicy,
    mut notify: impl FnMut(JoinProgress),
    mut connect: F,
) -> JoinRetryOutcome<T>
where
    F: FnMut(u32) -> Fut,
    Fut: std::future::Future<Output = Result<T, JoinAttemptError>>,
{
    let total = policy.total_attempts();
    let started = Instant::now();
    let mut attempts = 0u32;
    let mut last_error: Option<JoinAttemptError> = None;

    loop {
        if attempts >= total {
            break;
        }

        if attempts > 0 {
            let delay = JoinRetryPolicy::backoff_after(attempts);
            if !policy.budget.is_zero() && started.elapsed() + delay >= policy.budget {
                break;
            }
            if !delay.is_zero() {
                tracing::debug!("join: пауза {delay:?} перед попыткой {}", attempts + 1);
                tokio::time::sleep(delay).await;
            }
        }

        attempts = attempts.saturating_add(1);
        notify(JoinProgress::Attempt { attempt: attempts, total });

        let attempt_timeout = policy.timeout.max(Duration::from_millis(1));
        match tokio::time::timeout(attempt_timeout, connect(attempts)).await {
            Ok(Ok(value)) => {
                return JoinRetryOutcome::Connected { value, attempts };
            }
            Ok(Err(error)) => {
                if !error.is_retryable() {
                    tracing::warn!("join: попытка {attempts}/{total} — ошибка без ретрая: {error}");
                    return JoinRetryOutcome::Rejected { error, attempts };
                }
                tracing::warn!("join: попытка {attempts}/{total} не удалась: {error}");
                last_error = Some(error);
            }
            Err(_) => {
                tracing::warn!(
                    "join: попытка {attempts}/{total} — таймаут {attempt_timeout:?} \
                     (ENet connect + TP 00)"
                );
                last_error = Some(JoinAttemptError::Timeout(attempt_timeout));
            }
        }
    }

    let error = last_error.unwrap_or(JoinAttemptError::Timeout(policy.timeout));
    JoinRetryOutcome::Exhausted {
        error,
        attempts: attempts.max(1),
    }
}

/// `data_hash` variants for the handshake — in attempt order.
///
/// Explicit `--handshake-patches` — exactly one variant (the behavior before
/// M-NET follow-ups). Without it:
///
/// 1. full hash `SHA256(MAX.RES) ‖ SHA256(PATCHES.RES)` (assets path);
/// 2. only `SHA256(MAX.RES)` — a server whose `game_data` has no
///    `PATCHES.RES` expects a 64-character `data_hash`
///    (`DataHash::from_files`: a missing file = an empty part).
///
/// The second variant is added only if a local `PATCHES.RES`
/// exists and the hashes really differ.
pub fn handshake_data_hashes(config: &AppConfig) -> Result<Vec<DataHash>> {
    let patches = config.handshake_patches_path().to_path_buf();
    let full = DataHash::from_files(&config.max_res, &patches).with_context(|| {
        format!(
            "не удалось посчитать SHA256({} + {})",
            config.max_res.display(),
            patches.display()
        )
    })?;

    let mut hashes = vec![full];
    if config.handshake_patches.is_none() && full.patches_res.is_some() {
        hashes.push(DataHash {
            max_res: full.max_res,
            patches_res: None,
        });
    }

    Ok(hashes)
}

/// Connects to the session, iterating `data_hash` variants after `TP 03`.
///
/// Logic: [`retry_join`] for each variant. A `GameData` refusal is a reason to take
/// the next variant (the ticket is the same, a repeated `JoinGame` is not needed); any
/// other refusal (`Version`, `SessionTicket`, `Unknown`) — an immediate error, as is
/// exhaustion of the retries.
async fn start_with_hash_fallback<T, F, Fut>(
    base_config: &SessionConfig,
    hashes: &[DataHash],
    policy: JoinRetryPolicy,
    mut notify: impl FnMut(JoinProgress),
    mut connect: F,
) -> Result<(T, DataHash, u32), JoinAttemptError>
where
    F: FnMut(SessionConfig) -> Fut,
    Fut: std::future::Future<Output = Result<T, JoinAttemptError>>,
{
    let hash_count = hashes.len().max(1);
    let mut last_reject: Option<JoinAttemptError> = None;

    for (index, hash) in hashes.iter().enumerate() {
        let mut config = base_config.clone();
        config.data_hash = *hash;

        let connect_call = |_attempt: u32| {
            let config = config.clone();
            connect(config)
        };
        let outcome = retry_join(policy, &mut notify, connect_call).await;

        match outcome {
            JoinRetryOutcome::Connected { value, attempts } => {
                return Ok((value, *hash, attempts));
            }
            JoinRetryOutcome::Rejected { error, .. } if error.is_game_data_reject() => {
                if index + 1 < hashes.len() {
                    tracing::warn!(
                        "join: сервер отверг data_hash с PATCHES.RES (GameData) — \
                         пробую вариант без PATCHES.RES"
                    );
                    notify(JoinProgress::FallbackDataHash {
                        hash: index as u32 + 2,
                        hashes: hash_count as u32,
                    });
                    last_reject = Some(error);
                    continue;
                }
                return Err(error);
            }
            JoinRetryOutcome::Rejected { error, .. } => return Err(error),
            JoinRetryOutcome::Exhausted { error, .. } => return Err(error),
        }
    }

    Err(last_reject.unwrap_or(JoinAttemptError::Session(SessionError::Timeout {
        stage: "join",
        timeout: policy.timeout,
    })))
}

/// Retry of `Horizon JoinGame`: the server may not have registered the game yet.
///
/// Progress does not go to the window (JoinGame is gRPC, not "warming up ENet"): the line
/// `Session warming up — attempt N/M` describes `SessionHandle::start` attempts.
async fn join_horizon_with_retry(
    config: &AppConfig,
    options: &NetworkOptions,
) -> Result<GameJoined> {
    let game_id = options.game_id.clone();
    let password = options.password.clone();
    let connect = |_attempt: u32| {
        let config = config.clone();
        let game_id = game_id.clone();
        let password = password.clone();
        async move {
            connect_horizon(&config, &game_id, password.as_deref())
                .await
                .map_err(|error| JoinAttemptError::Horizon(format!("{error:#}")))
        }
    };

    match retry_join(options.join_retry, |_| {}, connect).await {
        JoinRetryOutcome::Connected { value, attempts } => {
            if attempts > 1 {
                tracing::info!("join: Horizon JoinGame прошёл с попытки {attempts}");
            }
            Ok(value)
        }
        JoinRetryOutcome::Exhausted { error, attempts } => Err(anyhow!(
            "не удалось войти в игру {game_id} за {attempts} попыт(ок): {error}"
        )),
        JoinRetryOutcome::Rejected { error, attempts } => Err(anyhow!(
            "Horizon JoinGame({game_id}) отклонён на попытке {attempts}: {error}"
        )),
    }
}

/// First-connection error with a hint about `--handshake-patches`.
fn join_failure(error: JoinAttemptError, tried_hashes: usize) -> anyhow::Error {
    if error.is_game_data_reject() && tried_hashes > 1 {
        anyhow!(
            "data_hash не принят сервером ни с PATCHES.RES, ни без него ({error}); \
             укажите --handshake-patches <PATCHES.RES, которым считает сервер>"
        )
    } else if error.is_game_data_reject() {
        anyhow!(
            "data_hash не принят сервером ({error}); сервер без PATCHES.RES ждёт хэш \
             только MAX.RES — проверьте --data-dir/--patches или укажите \
             --handshake-patches"
        )
    } else {
        anyhow!("{error}")
    }
}

/// Reports connection progress to the log and to the window (if there is a [`FrameSink`]).
fn report_join_progress(sink: &mut Option<&mut dyn FrameSink>, progress: JoinProgress) {
    match progress {
        JoinProgress::Attempt { attempt, total } => {
            tracing::warn!("join: сессия разогревается — попытка {attempt}/{total}");
        }
        JoinProgress::FallbackDataHash { hash, hashes } => {
            tracing::warn!(
                "join: вариантов data_hash {hashes}; пробую {hash} — только MAX.RES \
                 (PATCHES.RES-часть пуста)"
            );
        }
    }

    if let Some(sink) = sink.as_deref_mut() {
        sink.join_status(&progress.status_text());
    }
}

/// Opens the session: Horizon `JoinGame` + ENet/TP00, with retry and fallback.
async fn open_network_session(
    config: &AppConfig,
    options: &NetworkOptions,
    hashes: &[DataHash],
    sink: &mut Option<&mut dyn FrameSink>,
) -> Result<(GameJoined, SessionConfig, SessionHandle, DataHash, u32)> {
    let mut notify = |progress: JoinProgress| report_join_progress(sink, progress);

    let joined = join_horizon_with_retry(config, options).await?;
    let base_config = SessionConfig::from_joined(&joined, hashes[0]).with_player(
        &config.player_name,
        config.team_slot.min(3),
        config.clan,
    );

    let connect = |session_config: SessionConfig| async move {
        SessionHandle::start(session_config)
            .await
            .map_err(JoinAttemptError::from)
    };
    let (handle, hash, attempts) =
        start_with_hash_fallback(&base_config, hashes, options.join_retry, &mut notify, connect)
            .await
            .map_err(|error| join_failure(error, hashes.len()))?;

    // Reconnect (`--reconnect-*`) must repeat the handshake with the same hash
    // the server accepted the client with.
    let mut session_config = base_config;
    session_config.data_hash = hash;

    Ok((joined, session_config, handle, hash, attempts))
}


/// Reconnect attempt accounting — pure logic without the network.
///
/// Separates "how many and how long to try" from [`SessionHandle::start`] so that
/// the retry policy can be covered by tests with a mock connector.
#[derive(Debug, Clone)]
pub struct ReconnectTracker {
    policy: ReconnectPolicy,
    attempts: u32,
    last_error: Option<String>,
}

impl ReconnectTracker {
    /// A new tracker (before a disconnect — `attempts == 0`).
    pub fn new(policy: ReconnectPolicy) -> Self {
        Self {
            policy,
            attempts: 0,
            last_error: None,
        }
    }

    /// The policy the tracker was created with.
    pub fn policy(&self) -> ReconnectPolicy {
        self.policy
    }

    /// How many attempts have already been made.
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// How many attempts are left.
    pub fn remaining(&self) -> u32 {
        self.policy.attempts.saturating_sub(self.attempts)
    }

    /// Text of the last error (for the final report).
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Starts a round after [`SessionEvent::Disconnected`].
    ///
    /// Resets the previous round's counter and the last error.
    pub fn note_disconnect(&mut self) {
        self.attempts = 0;
        self.last_error = None;
    }

    /// Number of the next attempt or `None` when the attempts are exhausted/disabled.
    pub fn next_attempt(&mut self) -> Option<u32> {
        if self.attempts >= self.policy.attempts {
            return None;
        }
        self.attempts = self.attempts.saturating_add(1);
        Some(self.attempts)
    }

    /// Records the failure reason of the current attempt.
    pub fn record_failure(&mut self, error: impl std::fmt::Display) {
        self.last_error = Some(error.to_string());
    }
}

/// Outcome of a reconnect attempt.
pub enum ReconnectOutcome<T> {
    /// The transport was restored (a new session in `value`).
    Restored {
        /// A new ENet session (for the tracker — `()`).
        value: T,
        /// Number of the successful attempt.
        attempts: u32,
    },
    /// All attempts failed; `attempts` — how many were made.
    Exhausted {
        /// Number of attempts made.
        attempts: u32,
    },
    /// `ReconnectPolicy::attempts == 0`.
    Disabled,
}

/// Performs reconnect attempts through an arbitrary `connect`.
///
/// The function knows nothing about the network: tests pass a mock connector, production
/// code — [`SessionHandle::start`]. Each attempt is bounded by
/// [`ReconnectPolicy::timeout`], between failures — [`ReconnectPolicy::backoff`].
async fn reconnect_with<T, F, Fut>(
    tracker: &mut ReconnectTracker,
    mut connect: F,
) -> ReconnectOutcome<T>
where
    F: FnMut(u32) -> Fut,
    Fut: std::future::Future<Output = Result<T, SessionError>>,
{
    let policy = tracker.policy();

    if policy.attempts == 0 {
        tracing::warn!("reconnect: автоматическое переподключение выключено (0 попыток)");
        return ReconnectOutcome::Disabled;
    }

    loop {
        let Some(attempt) = tracker.next_attempt() else {
            let last = tracker.last_error().unwrap_or("нет данных");
            tracing::error!(
                "reconnect: попытки исчерпаны ({}/{}), последняя ошибка: {last}",
                tracker.attempts(),
                policy.attempts
            );
            return ReconnectOutcome::Exhausted {
                attempts: tracker.attempts(),
            };
        };

        tracing::warn!(
            "reconnect: retrying — попытка {attempt}/{} (таймаут {:?})",
            policy.attempts,
            policy.timeout
        );

        let attempt_timeout = policy.timeout.max(Duration::from_millis(1));
        match tokio::time::timeout(attempt_timeout, connect(attempt)).await {
            Ok(Ok(value)) => {
                tracing::info!(
                    "reconnect: ENet+TP00 восстановлены (попытка {attempt}/{})",
                    policy.attempts
                );
                return ReconnectOutcome::Restored {
                    value,
                    attempts: attempt,
                };
            }
            Ok(Err(error)) => {
                tracker.record_failure(&error);
                tracing::warn!(
                    "reconnect: попытка {attempt}/{} не удалась: {error}",
                    policy.attempts
                );
            }
            Err(_) => {
                tracker.record_failure(format!("таймаут {:?}", policy.timeout));
                tracing::warn!(
                    "reconnect: попытка {attempt}/{} — таймаут {:?}",
                    policy.attempts,
                    policy.timeout
                );
            }
        }

        if tracker.remaining() > 0 && !policy.backoff.is_zero() {
            tracing::debug!(
                "reconnect: пауза {:?} перед следующей попыткой",
                policy.backoff
            );
            tokio::time::sleep(policy.backoff).await;
        }
    }
}

/// Reconnects an existing session: ENet connect + `TP 00` again.
///
/// On success it replaces `handle` with a new session. The game state is not reset;
/// [`pump_events`] after returning publishes resume-54 and continues the barriers
/// (`max-brh-docs/protocol.md` §9).
async fn reconnect_session(
    handle: &mut SessionHandle,
    config: &SessionConfig,
    policy: ReconnectPolicy,
) -> ReconnectOutcome<()> {
    let mut tracker = ReconnectTracker::new(policy);
    tracker.note_disconnect();

    let outcome = reconnect_with(&mut tracker, |_attempt| {
        let config = config.clone();
        async move { SessionHandle::start(config).await }
    })
    .await;

    match outcome {
        ReconnectOutcome::Restored { value, attempts } => {
            *handle = value;
            ReconnectOutcome::Restored {
                value: (),
                attempts,
            }
        }
        ReconnectOutcome::Exhausted { attempts } => ReconnectOutcome::Exhausted { attempts },
        ReconnectOutcome::Disabled => ReconnectOutcome::Disabled,
    }
}

/// Frame sink of the network loop.
///
/// The library knows nothing about SDL: the binary behind the `sdl` feature passes
/// a wrapper over [`max_render::SdlWindow`], tests — a frame counter. `false`
/// means "close the window" and finish the session correctly.
///
/// The frame gets `&mut App` because the interactive client (`--ui`) handles the mouse
/// right here: unit selection and orders live in
/// [`crate::hud::HudState`] (outside `GameState`), and the assembled packets `38`/`08`/`18`
/// are taken and sent by [`flush_hud_orders`].
pub trait FrameSink {
    /// Show a frame of the current state; `false` — exit the loop.
    fn frame(&mut self, app: &mut App) -> bool;

    /// First-connection status (retry/fallback `data_hash`).
    ///
    /// Called before the first match frame, while `SessionHandle::start`
    /// is repeated. `false` means "close the window"; by default — no-op,
    /// so the headless smoke and test sinks change nothing.
    fn join_status(&mut self, _message: &str) -> bool {
        true
    }
}

/// Result of the network run (`--connect`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkReport {
    /// Session `game_id`.
    pub game_id: String,
    /// `TP 00` passed.
    pub handshake: bool,
    /// The lobby reached packet 34.
    pub lobby_complete: bool,
    /// Local team (slot from [`AppConfig::team_slot`]).
    pub local_team: TeamId,
    /// How many simulation ticks were run.
    pub ticks: u64,
    /// How many commands were applied.
    pub commands_applied: u64,
    /// How many turns were closed by barrier 52.
    pub turns_finished: u32,
    /// How many times the barrier did not wait for the peers.
    pub barrier_timeouts: u32,
    /// How many times the transport was restored after a disconnect.
    pub reconnects: u32,
    /// How many reconnect attempts were made in total.
    pub reconnect_attempts: u32,
    /// The transport was restored, but the match could not be continued (e.g.
    /// attempts exhausted/reconnect disabled). `false` after a successful
    /// resume-54 — the run continued.
    pub reconnect_pending_state: bool,
    /// `data_hash` the server accepted the client with (hex; 64 characters — a server
    /// without `PATCHES.RES`, 128 — full).
    pub data_hash: String,
    /// How many first-connection attempts (`Create game → Join → start`)
    /// were needed; `1` — the session was already ready.
    pub join_attempts: u32,
    /// Digest of the final state.
    pub digest: AppDigest,
}

/// Connects to Horizon and joins the game (`Hello` → `JoinGame`).
///
/// Returns [`GameJoined`]: the UDP session address, ticket and `game_id`.
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

/// In-game lobby (§8.2): returns node identifiers and settings.
#[derive(Debug, Clone, Default)]
struct LobbyInfo {
    handshake: bool,
    player_node: u16,
    host_node: u16,
    /// `Welcome.team_nodes` — which nodes took the team slots.
    team_nodes: [u16; 4],
    settings: Option<LobbyGameSettings>,
}

/// Runs the lobby `28 → 44 → 29 → 30 → 36 → 35 → 32 → 05 → 34`.
///
/// While the lobby is not finished, incoming game commands (e.g. 13 seed,
/// which the server sends right after the start) are already applied to [`App`] in the
/// order of receipt.
async fn run_lobby(
    app: &mut App,
    handle: &mut SessionHandle,
    session_config: &SessionConfig,
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
                    // The own slot is known locally: `Welcome` arrives before our 35.
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
                    // If the server relays foreign 35s — we update the slot map.
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
            Some(SessionEvent::Disconnected) => {
                tracing::warn!("reconnect: сервер разорвал ENet-сессию в лобби");
                match reconnect_session(handle, session_config, options.reconnect).await {
                    ReconnectOutcome::Restored { attempts, .. } => bail!(
                        "reconnect: транспорт восстановлен (ENet+TP00, попыток {attempts}), \
                         но повторный вход в лобби невозможен: сервер уже не в Remote_GameState=1 \
                         (max-brh-docs/protocol.md §9)"
                    ),
                    ReconnectOutcome::Exhausted { attempts } => bail!(
                        "reconnect: не удалось переподключиться за {attempts} попыт(ок) в лобби"
                    ),
                    ReconnectOutcome::Disabled => {
                        bail!("сервер отключил клиента в лобби (reconnect выключен)")
                    }
                }
            }
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

/// Network run without a window: [`run_network_with_sink`] with `sink = None`.
pub async fn run_network(app: &mut App, options: &NetworkOptions) -> Result<NetworkReport> {
    run_network_with_sink(app, options, None).await
}

/// Full network mode: Horizon → ENet → lobby → game loop.
///
/// `sink` gets a frame after each turn (an SDL window in the binary). If
/// [`FrameSink::frame`] returned `false`, the loop ends and the session closes
/// cleanly.
pub async fn run_network_with_sink(
    app: &mut App,
    options: &NetworkOptions,
    mut sink: Option<&mut dyn FrameSink>,
) -> Result<NetworkReport> {
    let config = app.config().clone();
    // The handshake counts PATCHES.RES at its own path: the server's `game_data` may
    // not contain the file, and then its hash part is empty. Assets are loaded
    // separately (`App::init_assets` → `assets::resolve_patches_res`), so
    // a nonexistent `--handshake-patches` does not leave the game without a map.
    //
    // Without an explicit `--handshake-patches` we try two variants (full → MAX.RES
    // only): this way Create game works against a server without PATCHES.RES without
    // a workaround flag.
    let hashes = handshake_data_hashes(&config)?;
    let (joined, session_config, mut handle, data_hash, join_attempts) =
        open_network_session(&config, options, &hashes, &mut sink).await?;

    let lobby = run_lobby(app, &mut handle, &session_config, &config, options).await?;

    // Local team: the slot occupied by our node in `Welcome.team_nodes`
    // (usually `--team`), otherwise — the requested slot.
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

    let stats = run_game_loop(
        app,
        &mut handle,
        &session_config,
        local_team,
        options,
        &mut sink,
    )
    .await?;
    handle.shutdown().await;

    if stats.reconnect_pending_state {
        tracing::warn!(
            "reconnect: транспорт восстановлен, но партия не продолжена \
             (попытки исчерпаны/реконнект выключен)"
        );
    }

    Ok(NetworkReport {
        game_id: joined.game_id,
        handshake: lobby.handshake,
        lobby_complete: true,
        local_team,
        ticks: app.ticks(),
        commands_applied: app.applied_commands(),
        turns_finished: stats.turns_finished,
        barrier_timeouts: stats.barrier_timeouts,
        reconnects: stats.reconnects,
        reconnect_attempts: stats.reconnect_attempts,
        reconnect_pending_state: stats.reconnect_pending_state,
        data_hash: data_hash.to_hex(),
        join_attempts,
        digest: app.digest(),
    })
}

/// Initializes the world, seed and team types from `LobbyGameSettings` (packet 30).
///
/// A port of the part of `GameManager_InitGame`/`Remote_SetupPlayers` without which
/// the CRC barrier 45 compares an empty map with the server's and "honestly" shows
/// a desync:
///
/// * `team_nodes[team] > 4` → own node = [`TeamKind::Player`], foreign =
///   [`TeamKind::Remote`], unoccupied slot = [`TeamKind::None`]
///   (`Remote_SetupPlayers`, `remote.cpp:672-734`);
/// * `world`/`seed`/resource levels and `ini_*` — from the lobby settings; the map
///   is generated by `setup::generate_map` (deterministically, as on the server);
/// * `ini_play_mode` decides whether the 00 barrier is needed every turn.
fn initialize_game(app: &mut App, lobby: &LobbyInfo, local_team: TeamId) {
    let Some(settings) = lobby.settings.as_ref() else {
        return;
    };

    let mut kinds = team_kinds_from_nodes(lobby.player_node, &lobby.team_nodes);
    if let Some(slot) = kinds.get_mut(local_team.0 as usize) {
        *slot = TeamKind::Player;
    }

    let planet = planet_from_settings(settings).unwrap_or(app.config().planet);
    let level =
        |raw: i32| ResourceLevel::new(raw.clamp(0, 2) as u8).unwrap_or(ResourceLevel::NORMAL);

    // Commands that arrived in the lobby (12 — mission supplies) belong to the "old"
    // state; after the world generation they must be returned, otherwise the team positions
    // would be lost. The starting point from 12 (if it has already arrived) goes into the generation.
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
        clans: [max_core::enums::TeamClan::Random; PLAYER_TEAM_COUNT],
        player_team: local_team,
        start_gold: settings.ini_start_gold.max(0) as u32,
        // A network game starts without mobilization/landing phases — the points come
        // from packet 30 (`supplies`), the composition is standard.
        deployments: Vec::new(),
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

/// `Remote_SetupPlayers` (`remote.cpp:705-729`): the team type by its node.
///
/// `team_nodes[team] > 4` — the slot is occupied by a client; own node → `Player`, foreign →
/// `Remote`. An unoccupied slot stays `None` (the client does not simulate the server's
/// AI teams and does not wait for them in the barriers).
fn team_kinds_from_nodes(player_node: u16, team_nodes: &[u16; 4]) -> [TeamKind; PLAYER_TEAM_COUNT] {
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

/// Planet from the lobby settings: `ini_world_index`, otherwise the `.WRL` resource name.
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

/// Default starting cell of a team (until packet 12 with the exact position).
fn default_team_start(team: usize) -> Point {
    const STARTS: [(i16, i16); 4] = [(20, 20), (90, 20), (20, 90), (90, 90)];
    let (x, y) = STARTS[team % STARTS.len()];
    Point::new(x, y)
}

#[derive(Debug, Default)]
struct GameLoopStats {
    turns_finished: u32,
    barrier_timeouts: u32,
    /// ENet session disconnects per run.
    disconnects: u32,
    /// Reconnect attempts made.
    reconnect_attempts: u32,
    /// Successful transport restorations and match continuations.
    reconnects: u32,
    /// The transport was restored, but the match state could not be restored
    /// (currently `false` after a successful resume-54; the field is kept for the report).
    reconnect_pending_state: bool,
}

/// Barrier repeat (`REMOTE_PING_TIME_PERIOD`).
const BARRIER_RETRY: Duration = DEFAULT_BARRIER_REPEAT;
/// Peer wait timeout (`REMOTE_RESPONSE_TIMEOUT`).
const BARRIER_TIMEOUT: Duration = DEFAULT_BARRIER_TIMEOUT;

/// Barrier FSM phase of the current turn (`max-brh-docs/barrier-flow.md` §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarrierPhase {
    /// 00 — entering the round.
    Begin,
    /// 53 — base totals at the start of the turn.
    Play,
    /// 52 — end of the turn.
    End,
    /// 54 — team state sync.
    Sync,
    /// 45 — CRC barrier.
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

/// Client barrier FSM of a turn: `00 → 53 → 06 → 52 → 54 → 45`.
///
/// The counters are separate (`Remote_FrameSyncCounter`, `Remote_TurnIndices`,
/// `Remote_StateSyncIndex`, `Remote_NextTurnIndices`), all pre-increment: the first
/// report is **1**. `GameManager_TurnCounter` in C++ starts at 1, while
/// `GameState::turn_counter` — at 0, so 53 carries `turn_counter + 1`.
#[derive(Debug)]
struct TurnBarrierFsm {
    local_team: TeamId,
    /// In `SimultaneousMoves` the 00 barrier is needed every turn, in `TurnBased` — once
    /// at the start (`game_manager.cpp:496`).
    simultaneous: bool,
    phase: BarrierPhase,
    begin_counter: u8,
    end_counter: u8,
    sync_index: u8,
    crc_index: u8,
    /// `GameManager_TurnCounter` for 53 (1-based).
    complex_turn: i32,
    /// Which `turn` the last 53 came with for each team.
    received_complex: [Option<i32>; PLAYER_TEAM_COUNT],
    /// Own world CRC in the current 45 round.
    crc_checksum: u16,
    /// 06 sent once per turn.
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

    /// C++ turn number (1-based `GameManager_TurnCounter`).
    fn cxx_turn(state: &GameState) -> i32 {
        state.turn_counter + 1
    }

    /// Peer teams: those whose barriers must be awaited (`TEAM_TYPE_REMOTE`).
    fn remote_teams<'a>(&'a self, state: &'a GameState) -> impl Iterator<Item = usize> + 'a {
        (0..PLAYER_TEAM_COUNT).filter(move |&team| state.teams[team].team_type == TeamKind::Remote)
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

    /// `Remote_WaitComplexTotals`: own team's 53 for the current turn.
    fn complex(&mut self, state: &GameState) -> Vec<Packet> {
        self.complex_turn = Self::cxx_turn(state);
        self.phase = BarrierPhase::Play;

        vec![complex_summary_for(
            state,
            self.local_team,
            self.complex_turn,
        )]
    }

    /// Remembers an incoming 53 (called for all teams, including own).
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

    /// Own team state for restoration after a reconnect.
    ///
    /// Packet 54 is the only "snapshot" in the protocol (`max-brh-docs/protocol.md` §8.5,
    /// §9): it carries the units, gold, research and id counters of one
    /// team. After a disconnect the client re-publishes **its own** state with
    /// the last completed `sync_index`: the server and peers apply it as
    /// authoritative (`Remote_ReceiveNetPacket_54`), while the peers' 54
    /// correct the remote teams on our side. There is no full map/SNAP in the
    /// protocol — see §9 of protocol.md about the limits.
    fn resume_state_sync(&self, state: &GameState) -> Vec<Packet> {
        state_sync_for(state, self.local_team, self.sync_index)
            .into_iter()
            .map(Packet::StateSync)
            .collect()
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

    /// CRC comparison after the 45 barrier: all peers have the same `crc16`.
    fn crc_matches(&self, state: &GameState) -> bool {
        self.remote_teams(state)
            .all(|team| state.team_data_crc16[team] == self.crc_checksum)
    }

    /// The current phase is complete (all peers sent the expected value).
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

    /// The turn is closed (`GameManager_TeamTurnFinish`): reset the local flag,
    /// the next 00 — only in `SimultaneousMoves`.
    fn on_turn_finished(&mut self) {
        self.local_finished_sent = false;
        self.phase = if self.simultaneous {
            BarrierPhase::Begin
        } else {
            BarrierPhase::Play
        };
    }
}

/// Outcome of waiting for a barrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitOutcome {
    /// The barrier is complete.
    Reached,
    /// The session ended.
    Stopped,
}

/// Deferred barrier send: retry every 3 s, timeout log after 30 s.
///
/// Unlike [`max_net::BarrierTracker`], the record does **not** disappear after
/// the timeout: C++ keeps pinging the lagging team, Rust logs and waits
/// (`max-brh-docs/barrier-flow.md` §3).
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

/// Drains session events; `false` — the session/loop is finished.
async fn pump_events(
    app: &mut App,
    handle: &mut SessionHandle,
    session_config: &SessionConfig,
    stats: &mut GameLoopStats,
    fsm: &mut TurnBarrierFsm,
    options: &NetworkOptions,
) -> Result<bool> {
    loop {
        match handle.next_event_timeout(options.event_wait).await {
            // A disconnect does not end the run silently: first an automatic
            // ENet connect + TP 00 per ReconnectPolicy, then resume-54 and
            // continuation of the barriers.
            Ok(Some(SessionEvent::Disconnected)) => {
                stats.disconnects = stats.disconnects.saturating_add(1);
                tracing::warn!("reconnect: сервер разорвал ENet-сессию");
                let outcome = reconnect_session(handle, session_config, options.reconnect).await;
                match outcome {
                    ReconnectOutcome::Restored { attempts, .. } => {
                        stats.reconnect_attempts =
                            stats.reconnect_attempts.saturating_add(attempts);
                        stats.reconnects = stats.reconnects.saturating_add(1);
                        stats.reconnect_pending_state = false;

                        // We do not reset the state: lockstep barriers prevent
                        // the match from moving ahead without our team, so
                        // the local turn/units are the continuation point. We re-publish
                        // our 54 (the server and peers apply the owner's state),
                        // the peers' 54 will correct the remote teams
                        // at the nearest SYNC; 53/52/45 are caught up by repeats.
                        let packets = fsm.resume_state_sync(app.state());
                        send_barrier(handle, &packets)
                            .await
                            .context("reconnect: публикация своего 54 после TP00")?;

                        tracing::warn!(
                            "reconnect: транспорт восстановлен (ENet+TP00, попыток {attempts}); \
                             партия продолжена, отправлен resume-54 команды {} (index {}, {} пакет(ов))",
                            fsm.local_team.0,
                            fsm.sync_index,
                            packets.len()
                        );
                    }
                    ReconnectOutcome::Exhausted { attempts } => {
                        stats.reconnect_attempts =
                            stats.reconnect_attempts.saturating_add(attempts);
                        stats.reconnect_pending_state = true;
                        tracing::error!(
                            "reconnect: переподключение не удалось за {attempts} попыт(ок) — \
                             прогон завершён"
                        );
                        return Ok(false);
                    }
                    ReconnectOutcome::Disabled => {
                        stats.reconnect_pending_state = true;
                        tracing::warn!("reconnect: автоматическое переподключение выключено");
                        return Ok(false);
                    }
                }
                // We continue draining events from the new session already; the barrier
                // in wait_barrier will repeat its packet as soon as the
                // three-second interval expires (for it, time runs from the last
                // send).
            }
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

/// Sends barrier packets without registering in the session tracker (the retries are done by
/// [`BarrierPending`], so there is no double tracking).
async fn send_barrier(handle: &SessionHandle, packets: &[Packet]) -> Result<()> {
    for packet in packets {
        handle
            .send_untracked(packet.clone())
            .await
            .context("отправка барьера")?;
    }
    Ok(())
}

/// Sends the orders/chat collected by HUD input (M4.3/M4.4) into the session.
///
/// The order is as in C++: first the local application (`Remote_ReceiveNetPacket_*`
/// for the own team), then multicast — `Packet::Command` via
/// [`SessionHandle::send`]. Packets 38/08/18 are not barriers, so the
/// session retry tracker does not register them (it has only 00/45/52).
///
/// Source — [`crate::hud::HudState::pending`]: they are collected by
/// [`crate::ui::handle_session_input`] from clicks (38 — movement with a path,
/// 08 — attack) and from the chat input line (18 — `Command::Chat`).
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

/// Per-frame barrier 01 (real-time pacing, §8.6).
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

/// Waits until all peers send the values of the current barrier.
///
/// While waiting: drains events (applying commands/54), repeats the barrier
/// packets via [`SessionHandle::send_untracked`] every 3 s, sends 01 and after
/// 30 s logs the timeout (but keeps waiting, like the Rust client in
/// `max-brh-docs/barrier-flow.md` §3).
async fn wait_barrier(
    app: &mut App,
    handle: &mut SessionHandle,
    session_config: &SessionConfig,
    stats: &mut GameLoopStats,
    fsm: &mut TurnBarrierFsm,
    frame_counter: &mut u8,
    options: &NetworkOptions,
    phase: BarrierPhase,
    packets: Vec<Packet>,
) -> Result<WaitOutcome> {
    let mut pending = BarrierPending::new(packets, Instant::now());

    loop {
        if !pump_events(app, handle, session_config, stats, fsm, options).await? {
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

/// Game loop of the lockstep client per the FSM `max-brh-docs/barrier-flow.md` §3.
async fn run_game_loop(
    app: &mut App,
    handle: &mut SessionHandle,
    session_config: &SessionConfig,
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

    // BEGIN (00): loading the match and every turn in SimultaneousMoves.
    let packets = fsm.begin(app.state_mut());
    send_barrier(handle, &packets).await?;
    if wait_barrier(
        app,
        handle,
        session_config,
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
        // PLAY (53): base totals at the start of the turn, then one tick and 06.
        let packets = fsm.complex(app.state());
        send_barrier(handle, &packets).await?;
        if wait_barrier(
            app,
            handle,
            session_config,
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
        tracing::debug!("барьер 53 (итоги баз) пройден: turn={}", fsm.complex_turn);

        // Turn ticks. Headless (no window or no `--turn-wait`) — one tick and
        // immediately 06: the turn closes as soon as the command queue is drained.
        // With a window and `--turn-wait N` — an interactive window: we run ticks and frames
        // until the player presses End Turn (or N seconds expire), otherwise
        // the human has no time to do anything — the turn lasts one frame.
        let interactive = sink.is_some() && !options.turn_wait.is_zero();
        let play_deadline = Instant::now() + options.turn_wait;

        loop {
            if !pump_events(app, handle, session_config, &mut stats, &mut fsm, options).await? {
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

            // Orders collected by input in this frame (38 — movement, 08 — attack):
            // local application + multicast into the session (M4.3).
            flush_hud_orders(app, handle).await?;

            let end_requested = app.hud_mut().take_end_turn_request();
            if end_requested {
                tracing::info!(
                    "HUD: End Turn нажат — закрываем ход команды {} (06 + 52)",
                    local_team.0
                );
            }

            if !interactive || end_requested || Instant::now() >= play_deadline {
                break;
            }
            tokio::time::sleep(options.tick_interval).await;
        }

        // 06 — exactly once per turn, when the local team has finished.
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
            session_config,
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

        // SYNC (54): the authoritative state of the own team.
        let packets = fsm.sync(app.state());
        send_barrier(handle, &packets).await?;
        if wait_barrier(
            app,
            handle,
            session_config,
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

        // CRC (45): the own CRC after the 54 correction, then the comparison with the peers.
        let packets = fsm.crc(app.state_mut());
        send_barrier(handle, &packets).await?;
        if wait_barrier(
            app,
            handle,
            session_config,
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

        // finish_turn: 52 is closed, all peers move to the next turn.
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

        // 00 of the next turn — only in SimultaneousMoves.
        if fsm.simultaneous {
            let packets = fsm.begin(app.state_mut());
            send_barrier(handle, &packets).await?;
            if wait_barrier(
                app,
                handle,
                session_config,
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

/// Handles one session event; `false` — the session has ended.
fn handle_session_event(
    app: &mut App,
    event: SessionEvent,
    stats: &mut GameLoopStats,
    fsm: &mut TurnBarrierFsm,
) -> Result<bool> {
    match event {
        SessionEvent::Packet(Packet::Command(command)) => {
            // Packet 13 — `Remote_RngSeed`. The world is already generated with this seed from
            // the settings 30 (`initialize_game`), and `GameManager_PopulateMapWithResources`
            // in C++ re-seeds the RNG and continues the stream. A repeated
            // `Randomizer_SetSeed` in the middle of a match would reset the stream and diverge
            // from the server simulation.
            if matches!(command, Command::Seed(_)) {
                tracing::debug!("13: seed уже применён при инициализации мира");
                return Ok(true);
            }

            // 53 — not only applying the base totals but also a fact for the barrier.
            if let Command::ComplexSummary(summary) = &command {
                fsm.note_complex(summary.team, summary.turn);
            }
            // 18 — chat: `apply_command` writes to `GameState::chat_log`, but
            // the player sees the HUD log, so we duplicate the line there too in
            // the C++ format (`Remote_ReceiveNetPacket_18`: `<team>: <text>`).
            if let Command::Chat(chat) = &command {
                let line = crate::hud::chat_line(chat.sender, &chat.message);
                app.hud_mut().push_log(line);
            }
            // 00 — the team is ALWAYS taken from the payload `team` field, not from the
            // ENet sender: the relay copy comes from the server, which has
            // no team of its own (`max-brh-docs/barrier-flow.md` §1, `remote.cpp:2313`).
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
            // A port of `Remote_ReceiveNetPacket_54`: we apply the authoritative
            // team state, otherwise the worlds diverge and the CRC barrier honestly
            // shows a desync.
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
            tracing::warn!(
                "сервер сообщил о десинке: команда {} param {}",
                team.0,
                param
            );
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

/// Diagnostic text for `TicketKind` (short, for logs).
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
    use max_core::enums::{TeamClan, UnitType};
    use max_core::input::{Packet18Chat, Packet45CrcBarrier};
    use max_core::team::TeamUnits;

    /// Synthetic world: local RED (Player), peers GREEN/BLUE (Remote).
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
        // The own counter is not yet agreed with the peers.
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

        // 06 — local end of the turn (exactly once).
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

        // finish_turn → the next turn starts with a clean 06.
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

        // Next turn: each counter has its own pre-increment (2).
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

        // 52 and 45 are different C++ arrays, not a shared counter.
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

        // 53 of the previous turn does not close the new one.
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

        // A new turn resets the flag: 06 will be sent again.
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

        // The timeout is logged once, but the record does not disappear — the barrier
        // keeps repeating, like a C++ ping.
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
        // Nodes 0..=4 are not clients (`Remote_SetupPlayers`: `> 4`).
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
        assert_eq!(
            planet_from_settings(&settings),
            None,
            "неизвестное имя → None"
        );
    }

    /// The relay copy comes from the server that has no team: the session
    /// event carries only the payload. The frame goes through the real wire codec.
    fn relayed(packet: &Packet) -> SessionEvent {
        let frame = packet.encode().expect("encode barrier");
        SessionEvent::Packet(Packet::decode(&frame).expect("decode relayed frame"))
    }

    /// Packet 00 from the server with `team = 1` is counted as team 1.
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
            app.state().frame_sync_counter[TeamId::GREEN.0 as usize],
            1,
            "team берётся из payload, а не из ENet-отправителя (сервер)"
        );
        assert!(fsm.reached(app.state()), "00-барьер проходит по team=1");
    }

    /// The incoming 18 is applied (`GameState::chat_log`) and gets into the HUD
    /// log in the C++ format (`Remote_ReceiveNetPacket_18`).
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

    /// State for a three-client scenario: the same world, one tank per
    /// team. Clients get a clone and change only `TeamKind`.
    fn three_team_state() -> (GameState, [max_core::UnitId; 3]) {
        use max_core::geo::Point;

        let mut state = GameState::new();
        let teams = [TeamId::RED, TeamId::GREEN, TeamId::BLUE];
        let mut ids: [max_core::UnitId; 3] = [max_core::UnitId::NO_ID; 3];

        for (index, team) in teams.iter().copied().enumerate() {
            state.team_units[team.0 as usize] = TeamUnits::with_clan(TeamClan::Random);
            ids[index] = state
                .spawn_unit(
                    UnitType::Tank,
                    team,
                    Point::new(20 + index as i16 * 5, 20),
                    0,
                )
                .expect("tank spawns");
        }
        state.play_mode = PlayMode::SimultaneousMoves;
        (state, ids)
    }

    struct SimClient {
        app: App,
        fsm: TurnBarrierFsm,
    }

    impl SimClient {
        fn new(base: &GameState, local: TeamId, remotes: &[TeamId]) -> Self {
            let mut app = App::new(AppConfig::default());
            *app.state_mut() = base.clone();
            let state = app.state_mut();
            for team in 0..PLAYER_TEAM_COUNT {
                state.teams[team].team_type = TeamKind::None;
            }
            state.teams[local.0 as usize].team_type = TeamKind::Player;
            for team in remotes {
                state.teams[team.0 as usize].team_type = TeamKind::Remote;
            }
            state.player_team = local;
            Self {
                app,
                fsm: TurnBarrierFsm::new(local, PlayMode::SimultaneousMoves),
            }
        }
    }

    /// Relay in a "star": the sender's packet is received by all other clients.
    fn relay(
        packets: &[Vec<Packet>],
        senders: &[usize],
        clients: &mut [SimClient],
        stats: &mut [GameLoopStats],
    ) {
        for &source in senders {
            for target in 0..clients.len() {
                if target == source {
                    continue;
                }
                for packet in &packets[source] {
                    let client = &mut clients[target];
                    handle_session_event(
                        &mut client.app,
                        relayed(packet),
                        &mut stats[target],
                        &mut client.fsm,
                    )
                    .expect("релейный пакет");
                }
            }
        }
    }

    fn send_all(
        clients: &mut [SimClient],
        stats: &mut [GameLoopStats],
        make: impl Fn(&mut SimClient) -> Vec<Packet>,
    ) -> Vec<Vec<Packet>> {
        let packets: Vec<Vec<Packet>> = clients.iter_mut().map(make).collect();
        let senders: Vec<usize> = (0..clients.len()).collect();
        relay(&packets, &senders, clients, stats);
        packets
    }

    /// P5: three clients, B disconnects, recovery via 54 and the turn continues.
    ///
    /// Mock transport — a deterministic relay without the network: while B is disconnected,
    /// its packets are not delivered; A/C have time to "move" and spoil B's unit
    /// copy (like a missed order). After reconnecting, B publishes its 54 —
    /// A/C accept the owner's state; the peer 54 of A corrects the copy
    /// of A's unit at B. The turn reaches 45, the CRC and turn_counter match.
    #[test]
    fn three_clients_survive_disconnect_and_converge_via_state_sync() {
        let (base, ids) = three_team_state();
        let mut clients = vec![
            SimClient::new(&base, TeamId::RED, &[TeamId::GREEN, TeamId::BLUE]),
            SimClient::new(&base, TeamId::GREEN, &[TeamId::RED, TeamId::BLUE]),
            SimClient::new(&base, TeamId::BLUE, &[TeamId::RED, TeamId::GREEN]),
        ];
        let mut stats = vec![
            GameLoopStats::default(),
            GameLoopStats::default(),
            GameLoopStats::default(),
        ];

        // 00: all three.
        send_all(&mut clients, &mut stats, |client| {
            client.fsm.begin(client.app.state_mut())
        });
        assert!(clients.iter().all(|client| client
            .fsm
            .reached(client.app.state())), "00 с 3 участниками");

        // 53: all three.
        send_all(&mut clients, &mut stats, |client| {
            client.fsm.complex(client.app.state())
        });
        assert!(clients.iter().all(|client| client
            .fsm
            .reached(client.app.state())), "53 с 3 участниками");

        // 06: local end of the turn, relay to the others.
        let mut finish_packets = Vec::new();
        for client in &mut clients {
            let finish = Command::FinishedTurn(Packet06FinishedTurn {
                team: client.fsm.local_team,
            });
            client.app.apply(finish.clone());
            client.fsm.mark_local_finished();
            finish_packets.push(vec![Packet::Command(finish)]);
        }
        let senders: Vec<usize> = (0..clients.len()).collect();
        relay(&finish_packets, &senders, &mut clients, &mut stats);

        // B (GREEN) disappears: A and C send 52, but B does not receive them, and its 52
        // is lost. Without B the barrier does not close — this is lockstep.
        let mut packets: Vec<Vec<Packet>> = Vec::new();
        for client in &mut clients {
            packets.push(client.fsm.end(client.app.state_mut()));
        }
        relay(&packets, &[0, 2], &mut clients, &mut stats);
        assert!(!clients[0].fsm.reached(clients[0].app.state()));
        assert!(!clients[2].fsm.reached(clients[2].app.state()));

        // While B is away, A/C change the copy of B's tank (a missed attack order).
        for index in [0usize, 2] {
            clients[index]
                .app
                .state_mut()
                .unit_mut(ids[1])
                .expect("tank B")
                .hits = 1;
        }
        // And B's copy of A's tank is stale.
        clients[1]
            .app
            .state_mut()
            .unit_mut(ids[0])
            .expect("tank A")
            .hits = 2;
        let b_tank_hits = clients[1]
            .app
            .state()
            .unit(ids[1])
            .expect("tank B")
            .hits;
        let a_tank_hits = clients[0]
            .app
            .state()
            .unit(ids[0])
            .expect("tank A")
            .hits;

        // Reconnect: B publishes its 54 (sync_index still 0), A/C apply it.
        let resume = clients[1].fsm.resume_state_sync(clients[1].app.state());
        assert!(
            resume.iter().any(
                |packet| matches!(packet, Packet::StateSync(sync) if sync.team == TeamId::GREEN)
            ),
            "resume-54 отправляет состояние B"
        );
        let mut resume_packets = vec![Vec::new(); clients.len()];
        resume_packets[1] = resume;
        relay(&resume_packets, &[1], &mut clients, &mut stats);
        for index in [0usize, 2] {
            assert_eq!(
                clients[index]
                    .app
                    .state()
                    .unit(ids[1])
                    .expect("tank B")
                    .hits,
                b_tank_hits,
                "54 владельца восстановил копию танка B"
            );
        }

        // B repeats its lost 52 (the same counter, without a new
        // pre-increment); A/C were already waiting for exactly it.
        let mut end_packets = vec![Vec::new(); clients.len()];
        end_packets[1] = packets[1].clone();
        relay(&end_packets, &[1], &mut clients, &mut stats);
        assert!(
            clients.iter().all(|client| client
                .fsm
                .reached(client.app.state())),
            "52 после возврата B: {:?}",
            clients
                .iter()
                .map(|client| (
                    client.fsm.local_team.0,
                    client.fsm.end_counter,
                    client.app.state().turn_indices,
                ))
                .collect::<Vec<_>>()
        );

        // 54 of all teams: B gets the current state of A/C, A/C — of B.
        send_all(&mut clients, &mut stats, |client| {
            client.fsm.sync(client.app.state())
        });
        assert!(clients.iter().all(|client| client
            .fsm
            .reached(client.app.state())), "54 с 3 участниками");
        assert_eq!(
            clients[1].app.state().unit(ids[0]).expect("tank A").hits,
            a_tank_hits,
            "54 A исправил устаревшую копию танка A у B"
        );

        // 45: each client's CRC matches everyone's.
        send_all(&mut clients, &mut stats, |client| {
            client.fsm.crc(client.app.state_mut())
        });
        assert!(clients.iter().all(|client| client
            .fsm
            .reached(client.app.state())), "45 с 3 участниками");
        for client in &clients {
            assert!(client.fsm.crc_matches(client.app.state()), "CRC 45");
        }

        let checksums: Vec<u16> = clients
            .iter()
            .map(|client| max_core::crc::state_checksum(client.app.state()))
            .collect();
        assert_eq!(checksums[0], checksums[1], "CRC A == B");
        assert_eq!(checksums[0], checksums[2], "CRC A == C");

        for client in &mut clients {
            client.app.state_mut().game_state = GameStateType::EndTurn;
            let _ = client.app.state_mut().finish_turn();
        }
        let turns: Vec<i32> = clients
            .iter()
            .map(|client| client.app.state().turn_counter)
            .collect();
        assert_eq!(turns, vec![1, 1, 1], "turn_counter догнал у всех");
    }

    /// Resume-54 after a reconnect: own team, the last sync_index and the full
    /// set of team units (including the economy tail in the last chunk).
    #[test]
    fn resume_state_sync_publishes_the_last_completed_index() {
        let (base, ids) = three_team_state();
        let client = SimClient::new(&base, TeamId::GREEN, &[TeamId::RED, TeamId::BLUE]);
        let mut fsm = TurnBarrierFsm::new(TeamId::GREEN, PlayMode::SimultaneousMoves);
        fsm.sync_index = 7;

        let packets = fsm.resume_state_sync(client.app.state());
        assert_eq!(packets.len(), 1, "один юнит — один last-чанк");
        match &packets[0] {
            Packet::StateSync(sync) => {
                assert_eq!(sync.team, TeamId::GREEN);
                assert_eq!(sync.sync_index, 7, "последний завершённый раунд");
                assert!(sync.is_last);
                assert_eq!(sync.units.len(), 1);
                assert_eq!(sync.units[0].0, ids[1], "только юниты своей команды");
            }
            other => panic!("expected 54, got {other:?}"),
        }
    }

    /// Two peers, the relay packets come from the server — barrier 00 converges.
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

        // The server relays 00(RED) to peer GREEN and 00(GREEN) to peer RED.
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
        assert_eq!(
            apps[0].state().frame_sync_counter[TeamId::GREEN.0 as usize],
            1
        );
        assert_eq!(
            apps[1].state().frame_sync_counter[TeamId::RED.0 as usize],
            1
        );
    }

    /// A disconnect in the event handler alone does not panic and ends
    /// the run (the retry is done by `pump_events`; the fallback is checked here).
    #[test]
    fn disconnect_event_stops_the_handler_without_panicking() {
        let local = TeamId::RED;
        let mut app = App::new(AppConfig::default());
        *app.state_mut() = synthetic_state(local, &[TeamId::GREEN]);
        let mut fsm = TurnBarrierFsm::new(local, PlayMode::TurnBased);
        let mut stats = GameLoopStats::default();

        let keep_going =
            handle_session_event(&mut app, SessionEvent::Disconnected, &mut stats, &mut fsm)
                .expect("разрыв — не ошибка обработчика");

        assert!(!keep_going, "Disconnected завершает обработку");
    }

    #[test]
    fn reconnect_tracker_starts_a_new_round_after_disconnect() {
        let mut tracker = ReconnectTracker::new(ReconnectPolicy {
            attempts: 2,
            timeout: Duration::from_secs(1),
            backoff: Duration::ZERO,
        });
        assert_eq!(tracker.next_attempt(), Some(1));
        assert_eq!(tracker.next_attempt(), Some(2));
        assert_eq!(tracker.next_attempt(), None, "лимит попыток");
        tracker.record_failure("boom");

        // A new disconnect — a new round, the previous error is not carried over.
        tracker.note_disconnect();
        assert_eq!(tracker.attempts(), 0);
        assert!(tracker.last_error().is_none());
        assert_eq!(tracker.next_attempt(), Some(1));
    }

    /// Mock session: the first attempt fails, the second succeeds — the retry logic
    /// restores the transport and reports the number of the successful attempt.
    #[tokio::test]
    async fn disconnect_retry_reconnects_on_the_second_attempt() {
        let mut tracker = ReconnectTracker::new(ReconnectPolicy {
            attempts: 3,
            timeout: Duration::from_secs(1),
            backoff: Duration::ZERO,
        });
        tracker.note_disconnect();

        let mut calls = 0u32;
        let outcome = reconnect_with(&mut tracker, |attempt| {
            calls += 1;
            let result = if attempt == 1 {
                Err(SessionError::Closed)
            } else {
                Ok(())
            };
            async move { result }
        })
        .await;

        assert!(
            matches!(
                outcome,
                ReconnectOutcome::Restored {
                    value: (),
                    attempts: 2
                }
            ),
            "успех на второй попытке"
        );
        assert_eq!(calls, 2);
        assert_eq!(tracker.attempts(), 2);
    }

    /// All attempts fail: `Exhausted` with the attempt count, without panics.
    #[tokio::test]
    async fn disconnect_retry_reports_exhaustion_without_panicking() {
        let mut tracker = ReconnectTracker::new(ReconnectPolicy {
            attempts: 2,
            timeout: Duration::from_secs(1),
            backoff: Duration::ZERO,
        });
        tracker.note_disconnect();

        let mut calls = 0u32;
        let outcome = reconnect_with(&mut tracker, |_attempt| {
            calls += 1;
            async { Err::<(), SessionError>(SessionError::Closed) }
        })
        .await;

        assert!(matches!(
            outcome,
            ReconnectOutcome::Exhausted { attempts: 2 }
        ));
        assert_eq!(calls, 2);
        assert_eq!(tracker.last_error(), Some("session channel is closed"));
    }

    /// `attempts = 0` — reconnect is disabled, the connector is not called.
    #[tokio::test]
    async fn reconnect_zero_attempts_is_disabled_without_connecting() {
        let mut tracker = ReconnectTracker::new(ReconnectPolicy {
            attempts: 0,
            timeout: Duration::from_secs(1),
            backoff: Duration::ZERO,
        });
        tracker.note_disconnect();

        let mut calls = 0u32;
        let outcome = reconnect_with(&mut tracker, |_attempt| {
            calls += 1;
            async { Ok::<(), SessionError>(()) }
        })
        .await;

        assert!(matches!(outcome, ReconnectOutcome::Disabled));
        assert_eq!(calls, 0);
        assert_eq!(tracker.attempts(), 0);
    }

    /// An attempt timeout does not hang the run and goes into `last_error`.
    #[tokio::test]
    async fn reconnect_attempt_timeout_is_reported() {
        let mut tracker = ReconnectTracker::new(ReconnectPolicy {
            attempts: 1,
            timeout: Duration::from_millis(5),
            backoff: Duration::ZERO,
        });
        tracker.note_disconnect();

        let outcome = reconnect_with(&mut tracker, |_attempt| async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok::<(), SessionError>(())
        })
        .await;

        assert!(matches!(
            outcome,
            ReconnectOutcome::Exhausted { attempts: 1 }
        ));
        assert!(
            tracker.last_error().unwrap_or_default().contains("таймаут"),
            "last_error: {:?}",
            tracker.last_error()
        );
    }

    // -----------------------------------------------------------------------
    // M-NET follow-ups: first-connection retry and data_hash fallback
    // -----------------------------------------------------------------------

    fn reject(reason: RejectReason) -> JoinAttemptError {
        JoinAttemptError::Session(SessionError::Handshake(HandshakeError::Rejected {
            reason,
            reason_text: reason.text().to_string(),
        }))
    }

    fn join_policy(attempts: u32, timeout: Duration) -> JoinRetryPolicy {
        JoinRetryPolicy {
            attempts,
            timeout,
            budget: Duration::from_secs(30),
        }
    }

    fn max_patches_hashes() -> [DataHash; 2] {
        let max = [0x11u8; 32];
        let patches = [0x22u8; 32];
        [
            DataHash::from_digests(max, patches),
            DataHash {
                max_res: Some(max),
                patches_res: None,
            },
        ]
    }

    fn session_config(hash: DataHash) -> SessionConfig {
        SessionConfig::new("127.0.0.1", 32000, "ticket", hash)
    }

    #[test]
    fn join_backoff_is_one_two_three_five_eight_eleven() {
        assert_eq!(JoinRetryPolicy::backoff_after(0), Duration::ZERO);
        assert_eq!(
            (1..=6)
                .map(JoinRetryPolicy::backoff_after)
                .collect::<Vec<_>>(),
            vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(3),
                Duration::from_secs(5),
                Duration::from_secs(8),
                Duration::from_secs(11),
            ]
        );
        assert_eq!(
            JoinRetryPolicy::backoff_after(9),
            Duration::from_secs(11),
            "после таблицы задержка не растёт"
        );
        assert_eq!(
            JoinRetryPolicy {
                attempts: 0,
                ..Default::default()
            }
            .total_attempts(),
            1,
            "0 попыток = одна попытка без ретраев"
        );
    }

    /// Mock connector: the first attempt fails, the second succeeds. Time is
    /// controlled by `start_paused`: between attempts exactly backoff (1 s).
    #[tokio::test(start_paused = true)]
    async fn join_retry_succeeds_on_the_second_attempt_after_backoff() {
        let started = tokio::time::Instant::now();
        let mut calls = 0u32;

        let outcome = retry_join(join_policy(3, Duration::from_secs(2)), |_| {}, |attempt| {
            calls += 1;
            async move {
                if attempt == 1 {
                    Err(JoinAttemptError::Timeout(Duration::from_secs(2)))
                } else {
                    Ok(42u32)
                }
            }
        })
        .await;

        assert!(
            matches!(
                outcome,
                JoinRetryOutcome::Connected {
                    value: 42,
                    attempts: 2
                }
            ),
            "успех на второй попытке"
        );
        assert_eq!(calls, 2);
        assert_eq!(
            started.elapsed(),
            Duration::from_secs(1),
            "мок отвечает мгновенно; 1 с — backoff после первой неудачи"
        );
    }

    /// The attempt timeout is bounded by the policy and does not hang the run: 2 s + 1 s
    /// backoff + 2 s = 5 s of virtual time for two attempts.
    #[tokio::test(start_paused = true)]
    async fn join_retry_attempt_timeout_is_bounded_by_the_policy() {
        let started = tokio::time::Instant::now();

        let outcome = retry_join(join_policy(2, Duration::from_secs(2)), |_| {}, |_attempt| async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok::<(), JoinAttemptError>(())
        })
        .await;

        match outcome {
            JoinRetryOutcome::Exhausted { error, attempts } => {
                assert_eq!(attempts, 2);
                assert!(
                    matches!(error, JoinAttemptError::Timeout(timeout) if timeout == Duration::from_secs(2))
                );
            }
            _ => panic!("ожидалось исчерпание попыток"),
        }
        assert_eq!(started.elapsed(), Duration::from_secs(5));
    }

    /// `TP 03 GameData` → one reconnect with the MAX.RES-only hash (the same
    /// ticket), the session starts. It is the full hash that is rejected.
    #[tokio::test]
    async fn game_data_reject_falls_back_to_the_max_only_hash() {
        let hashes = max_patches_hashes();
        let base = session_config(hashes[0]);
        let mut tried: Vec<String> = Vec::new();

        let outcome: Result<((), DataHash, u32), JoinAttemptError> = start_with_hash_fallback(
            &base,
            &hashes,
            join_policy(2, Duration::from_secs(1)),
            |_| {},
            |config| {
                tried.push(config.data_hash.to_hex());
                let full = config.data_hash.patches_res.is_some();
                async move {
                    if full {
                        Err(reject(RejectReason::GameData))
                    } else {
                        Ok(())
                    }
                }
            },
        )
        .await;

        let (_, accepted, attempts) = outcome.expect("fallback-хэш принят");
        assert_eq!(attempts, 1, "fallback — тоже первая попытка");
        assert_eq!(accepted.patches_res, None);
        assert_eq!(tried.len(), 2, "полный хэш, затем MAX-only");
        assert_eq!(tried[0].len(), 128);
        assert_eq!(tried[1].len(), 64);
        assert_eq!(tried[0][..64], tried[1][..], "MAX.RES-часть та же");
        assert!(
            base.data_hash.patches_res.is_some(),
            "базовый конфиг не мутируется"
        );
    }

    /// A refusal for another reason (`Version`) — no fallback and no retry.
    #[tokio::test]
    async fn version_reject_is_neither_retried_nor_fallen_back() {
        let hashes = max_patches_hashes();
        let base = session_config(hashes[0]);
        let mut calls = 0u32;

        let outcome: Result<((), DataHash, u32), JoinAttemptError> = start_with_hash_fallback(
            &base,
            &hashes,
            join_policy(3, Duration::from_secs(1)),
            |_| {},
            |_config| {
                calls += 1;
                async move { Err(reject(RejectReason::Version)) }
            },
        )
        .await;

        assert!(outcome.is_err(), "Version не лечится сменой data_hash");
        assert_eq!(calls, 1, "ретраев и fallback нет");
        assert!(!outcome.unwrap_err().is_game_data_reject());
    }

    /// The network loop waits for the `PATCHES.RES` variant only without an explicit flag.
    #[test]
    fn handshake_hashes_try_full_then_max_only_by_default() {
        let dir = std::env::temp_dir().join(format!(
            "max_app_join_hashes_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("MAX.RES"), b"max-res").expect("MAX.RES");
        std::fs::write(dir.join("PATCHES.RES"), b"patches").expect("PATCHES.RES");

        let config = AppConfig {
            max_res: dir.join("MAX.RES"),
            patches_res: dir.join("PATCHES.RES"),
            ..Default::default()
        };
        let hashes = handshake_data_hashes(&config).expect("хэши");
        assert_eq!(hashes.len(), 2);
        assert!(hashes[0].patches_res.is_some());
        assert_eq!(hashes[0].to_hex().len(), 128);
        assert!(hashes[1].patches_res.is_none());
        assert_eq!(hashes[1].to_hex().len(), 64);
        assert_eq!(hashes[0].max_res, hashes[1].max_res);

        // Explicit --handshake-patches — exactly one variant, no guessing.
        let explicit = AppConfig {
            handshake_patches: Some(dir.join("nonexistent.res")),
            ..config.clone()
        };
        let explicit_hashes = handshake_data_hashes(&explicit).expect("хэши");
        assert_eq!(explicit_hashes.len(), 1);
        assert!(explicit_hashes[0].patches_res.is_none());

        // No local PATCHES.RES — the full variant is already MAX-only.
        let without_patches = AppConfig {
            patches_res: dir.join("nonexistent.res"),
            ..config
        };
        let max_only = handshake_data_hashes(&without_patches).expect("хэши");
        assert_eq!(max_only.len(), 1);
        assert_eq!(max_only[0].to_hex().len(), 64);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn join_progress_status_names_the_attempt_and_the_fallback() {
        assert_eq!(
            JoinProgress::Attempt {
                attempt: 2,
                total: 6
            }
            .status_text(),
            "Session warming up — attempt 2/6"
        );
        let fallback = JoinProgress::FallbackDataHash { hash: 2, hashes: 2 }.status_text();
        assert!(fallback.contains("MAX.RES-only"));
        assert!(fallback.contains("2/2"));
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
