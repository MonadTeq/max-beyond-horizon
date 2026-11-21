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
//!
//! # Реконнект
//!
//! `SessionEvent::Disconnected` запускает [`ReconnectPolicy`]: клиент заново
//! делает ENet connect + `TP 00` тем же тикетом (с таймаутом и числом попыток
//! из `--reconnect-attempts`/`--reconnect-timeout`), без повторного лобби.
//! Локальное lockstep-состояние не сбрасывается: пока клиент был offline,
//! пиры/сервер ждали его барьер. После `TP 00` [`TurnBarrierFsm::resume_state_sync`]
//! публикует 54 своей команды (юниты+экономика, последний `sync_index`),
//! встречные 54 пиров исправляют удалённые команды, затем доигрываются
//! 52/54/45 и `finish_turn`; `reconnect_pending_state=false`.
//!
//! Пределы (`docs/protocol.md` §9): SNAP-пакета нет, 54 покрывает только
//! юнитов/экономику одной команды, пропущенные приказы других не
//! replay-ятся, C++-пиры исключают команду через 30 с. Лобби-реконнект не
//! поддерживается (сервер уже не в `Remote_GameState=1`).
//!
//! # Ретрай первого подключения и `data_hash` (M-NET-доводки)
//!
//! После `Create game` сессия headless поднимается 10–15 с, и первый
//! `ENet connect` падает (`connect timed out after 10s`). [`open_network_session`]
//! повторяет `SessionHandle::start` по [`JoinRetryPolicy`] (backoff
//! 1,2,3,5,8,11 с, окно ~30 с), не возвращая игрока в список игр; прогресс
//! уходит в [`FrameSink::join_status`] («Session warming up — attempt N/M»).
//!
//! `data_hash` согласуется без `--handshake-patches`: [`handshake_data_hashes`]
//! даёт полный хэш (`MAX.RES` + `PATCHES.RES`), затем хэш только `MAX.RES`;
//! после `TP 03 reason=GameData` клиент один раз переподключается со вторым
//! вариантом (тикет тот же — Horizon `JoinGame` не повторяется). Любой другой
//! `TP 03` (`Version`, `SessionTicket`) fallback не запускает.

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
    /// Политика переподключения при разрыве ENet-сессии.
    pub reconnect: ReconnectPolicy,
    /// Политика ретрая **первого** подключения (Create game → Join → start):
    /// сессия headless греется 10–15 с, и ENet connect в это окно падает.
    pub join_retry: JoinRetryPolicy,
    /// Сколько ждать ввода игрока в фазе PLAY, прежде чем закрыть ход.
    ///
    /// `Duration::ZERO` (по умолчанию) — headless-поведение: тик, кадр и сразу
    /// `06 + 52`, ход не ждёт человека (так работают `--connect`, live-smoke и
    /// тесты). `--turn-wait N` включает интерактивное окно: с SDL-окном цикл
    /// крутит тики/кадры и держит ход открытым до нажатия **End Turn** или до
    /// истечения `N` секунд.
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
    /// Параметры для игры `game_id` и таймаутами по умолчанию.
    pub fn new(game_id: impl Into<String>) -> Self {
        Self {
            game_id: game_id.into(),
            ..Self::default()
        }
    }
}

/// Сколько попыток переподключения делать по умолчанию.
pub const DEFAULT_RECONNECT_ATTEMPTS: u32 = 3;
/// Таймаут одной попытки: ENet connect + `TP 00`.
pub const DEFAULT_RECONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Пауза между неудачными попытками.
pub const DEFAULT_RECONNECT_BACKOFF: Duration = Duration::from_secs(1);

/// Политика переподключения после разрыва ENet-сессии.
///
/// `attempts = 0` выключает автоматический реконнект. Транспорт
/// восстанавливается через ENet connect + `TP 00`; игровое состояние
/// продолжается с локальной точки lockstep и догоняется resume-54
/// (`docs/protocol.md` §9). SNAP/полного снимка по-прежнему нет.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectPolicy {
    /// Максимальное число попыток (`0` — не переподключаться).
    pub attempts: u32,
    /// Таймаут одной попытки (ENet connect + `TP 00`).
    pub timeout: Duration,
    /// Пауза между неудачными попытками.
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

/// Сколько попыток первого подключения делать по умолчанию.
///
/// Это **не** реконнект: после `Create game` сессия headless поднимается
/// 10–15 с, и `ENet connect` до этого просто не отвечает
/// (`connect timed out after 10s`). Ретрай делает Create → InSession
/// самодостаточным, без «создать → подождать → Join из списка».
pub const DEFAULT_JOIN_RETRY_ATTEMPTS: u32 = 6;
/// Таймаут одной попытки первого подключения (ENet connect + `TP 00`).
pub const DEFAULT_JOIN_RETRY_TIMEOUT: Duration = Duration::from_secs(4);
/// Общее окно ретраев: новые попытки после него не начинаются.
pub const DEFAULT_JOIN_RETRY_BUDGET: Duration = Duration::from_secs(30);

/// Задержки перед повторными попытками: 1, 2, 3, 5, 8, 11 с (сумма 30 с).
pub const JOIN_RETRY_BACKOFF: [Duration; 6] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(3),
    Duration::from_secs(5),
    Duration::from_secs(8),
    Duration::from_secs(11),
];

/// Политика ретрая первого подключения к сессии (`Create game → Join → start`).
///
/// `attempts` — **всего** попыток (`0` трактуется как одна попытка без
/// ретраев); `timeout` — таймаут одной попытки (`ENet connect + TP 00`);
/// `budget` — окно, после которого новые попытки не начинаются (по умолчанию
/// 30 с). Задержки между попытками — [`JOIN_RETRY_BACKOFF`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JoinRetryPolicy {
    /// Всего попыток подключения (`0` — одна, без ретраев).
    pub attempts: u32,
    /// Таймаут одной попытки (ENet connect + `TP 00`).
    pub timeout: Duration,
    /// Общее окно ретраев; `Duration::ZERO` — без ограничения.
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
    /// Задержка перед следующей попыткой после `failures` неудач.
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

    /// Всего попыток с учётом `0` = «одна, без ретраев».
    pub fn total_attempts(self) -> u32 {
        self.attempts.max(1)
    }
}

/// Прогресс первого подключения — для строки статуса UI и лога.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinProgress {
    /// Очередная попытка `attempt` из `total` (сессия греется).
    Attempt {
        /// Номер попытки (1-based).
        attempt: u32,
        /// Всего попыток по политике.
        total: u32,
    },
    /// Сервер отверг `data_hash` (`TP 03`, reason `GameData`) — клиент
    /// переподключается с fallback-хэшем (только `MAX.RES`).
    FallbackDataHash {
        /// Номер варианта хэша (1-based), с которым сейчас подключаемся.
        hash: u32,
        /// Сколько вариантов всего.
        hashes: u32,
    },
}

impl JoinProgress {
    /// Строка статуса для окна/меню (английский — как весь UI оригинала).
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

/// Ошибка одной попытки подключения (Horizon JoinGame или ENet + `TP 00`).
#[derive(Debug, thiserror::Error)]
pub enum JoinAttemptError {
    /// Horizon `JoinGame` не ответил/ответил ошибкой.
    #[error("Horizon JoinGame: {0}")]
    Horizon(String),
    /// ENet connect / `TP 00`.
    #[error(transparent)]
    Session(#[from] SessionError),
    /// Внешний таймаут попытки ([`JoinRetryPolicy::timeout`]).
    #[error("join timed out after {0:?}")]
    Timeout(Duration),
}

impl JoinAttemptError {
    /// Стоит ли повторять попытку: таймаут/транспорт — да, `TP 03` — нет.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Horizon(_) | Self::Timeout(_) => true,
            Self::Session(error) => match error {
                // Версия/тикет/данные и битый TP 00 — ответ сервера, повтор
                // ничего не изменит.
                SessionError::Handshake(_) | SessionError::Closed | SessionError::Wire(_) => false,
                // Таймаут connect/handshake, разрыв, ENet, DNS — транспортные.
                SessionError::Timeout { .. }
                | SessionError::Disconnected { .. }
                | SessionError::Enet(_)
                | SessionError::Resolution { .. } => true,
            },
        }
    }

    /// `TP 03` с `reason = GameData` (сервер ждёт другой `data_hash`).
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

/// Итог ретрая одного этапа подключения.
enum JoinRetryOutcome<T> {
    /// Подключение прошло на попытке `attempts`.
    Connected {
        /// Результат коннектора.
        value: T,
        /// Номер успешной попытки.
        attempts: u32,
    },
    /// Попытки кончились (ошибки были retryable).
    Exhausted {
        /// Последняя ошибка.
        error: JoinAttemptError,
        /// Сколько попыток сделано.
        attempts: u32,
    },
    /// Ошибка, которую повторять бессмысленно (`TP 03` и т.п.).
    Rejected {
        /// Ошибка сервера.
        error: JoinAttemptError,
        /// Сколько попыток сделано.
        attempts: u32,
    },
}

/// Выполняет попытки подключения с backoff и общим окном.
///
/// Функция не знает, что именно подключается: тесты передают мок-коннектор, а
/// боевой код — Horizon `JoinGame` и [`SessionHandle::start`]. Каждая попытка
/// ограничена [`JoinRetryPolicy::timeout`], паузы — [`JOIN_RETRY_BACKOFF`],
/// новые попытки после [`JoinRetryPolicy::budget`] не начинаются.
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

/// Варианты `data_hash` для хендшейка — в порядке попыток.
///
/// Явный `--handshake-patches` — ровно один вариант (поведение до M-NET-
/// доводок). Без него:
///
/// 1. полный хэш `SHA256(MAX.RES) ‖ SHA256(PATCHES.RES)` (assets-путь);
/// 2. только `SHA256(MAX.RES)` — сервер, чей `game_data` не содержит
///    `PATCHES.RES`, ждёт 64-символьный `data_hash`
///    (`DataHash::from_files`: отсутствующий файл = пустая часть).
///
/// Второй вариант добавляется только если локальный `PATCHES.RES`
/// существует и хэши действительно различаются.
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

/// Подключается к сессии, перебирая варианты `data_hash` после `TP 03`.
///
/// Логика: [`retry_join`] по каждому варианту. Отказ `GameData` — повод взять
/// следующий вариант (тикет тот же, повторный `JoinGame` не нужен); любой
/// другой отказ (`Version`, `SessionTicket`, `Unknown`) — сразу ошибка, как и
/// исчерпание ретраев.
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

/// Ретрай `Horizon JoinGame`: сервер мог ещё не зарегистрировать игру.
///
/// Прогресс в окно не идёт (JoinGame — gRPC, а не «прогрев ENet»): строка
/// `Session warming up — attempt N/M` описывает попытки `SessionHandle::start`.
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

/// Ошибка первого подключения с подсказкой про `--handshake-patches`.
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

/// Сообщает прогресс подключения в лог и в окно (если есть [`FrameSink`]).
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

/// Открывает сессию: Horizon `JoinGame` + ENet/TP00, с ретраем и fallback.
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

    // Реконнект (`--reconnect-*`) должен повторять хэндшейк тем же хэшем,
    // которым сервер принял клиента.
    let mut session_config = base_config;
    session_config.data_hash = hash;

    Ok((joined, session_config, handle, hash, attempts))
}


/// Учёт попыток переподключения — чистая логика без сети.
///
/// Отделяет «сколько и как долго пытаться» от [`SessionHandle::start`], чтобы
/// retry-политику можно было покрыть тестами с мок-коннектором.
#[derive(Debug, Clone)]
pub struct ReconnectTracker {
    policy: ReconnectPolicy,
    attempts: u32,
    last_error: Option<String>,
}

impl ReconnectTracker {
    /// Новый трекер (до разрыва — `attempts == 0`).
    pub fn new(policy: ReconnectPolicy) -> Self {
        Self {
            policy,
            attempts: 0,
            last_error: None,
        }
    }

    /// Политика, с которой создан трекер.
    pub fn policy(&self) -> ReconnectPolicy {
        self.policy
    }

    /// Сколько попыток уже сделано.
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Сколько попыток ещё осталось.
    pub fn remaining(&self) -> u32 {
        self.policy.attempts.saturating_sub(self.attempts)
    }

    /// Текст последней ошибки (для итогового репорта).
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Начинает раунд после [`SessionEvent::Disconnected`].
    ///
    /// Сбрасывает счётчик прошлого раунда и последнюю ошибку.
    pub fn note_disconnect(&mut self) {
        self.attempts = 0;
        self.last_error = None;
    }

    /// Номер следующей попытки или `None`, когда попытки исчерпаны/выключены.
    pub fn next_attempt(&mut self) -> Option<u32> {
        if self.attempts >= self.policy.attempts {
            return None;
        }
        self.attempts = self.attempts.saturating_add(1);
        Some(self.attempts)
    }

    /// Запоминает причину неудачи текущей попытки.
    pub fn record_failure(&mut self, error: impl std::fmt::Display) {
        self.last_error = Some(error.to_string());
    }
}

/// Итог попытки переподключения.
pub enum ReconnectOutcome<T> {
    /// Транспорт восстановлен (новая сессия в `value`).
    Restored {
        /// Новая ENet-сессия (для трекера — `()`).
        value: T,
        /// Номер успешной попытки.
        attempts: u32,
    },
    /// Все попытки неудачны; `attempts` — сколько сделано.
    Exhausted {
        /// Число сделанных попыток.
        attempts: u32,
    },
    /// `ReconnectPolicy::attempts == 0`.
    Disabled,
}

/// Выполняет попытки переподключения через произвольный `connect`.
///
/// Функция не знает про сеть: тесты передают мок-коннектор, боевой код —
/// [`SessionHandle::start`]. Каждая попытка ограничена
/// [`ReconnectPolicy::timeout`], между неудачами — [`ReconnectPolicy::backoff`].
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

/// Переподключает существующую сессию: ENet connect + `TP 00` заново.
///
/// При успехе заменяет `handle` новой сессией. Состояние игры не сбрасывается;
/// [`pump_events`] после возврата публикует resume-54 и продолжает барьеры
/// (`docs/protocol.md` §9).
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

    /// Статус первого подключения (ретрай/fallback `data_hash`).
    ///
    /// Вызывается до первого кадра партии, пока `SessionHandle::start`
    /// повторяется. `false` означает «закрыть окно»; по умолчанию — no-op,
    /// поэтому headless-смоук и тестовые приёмники ничего не меняют.
    fn join_status(&mut self, _message: &str) -> bool {
        true
    }
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
    /// Сколько раз транспорт восстанавливался после разрыва.
    pub reconnects: u32,
    /// Сколько попыток переподключения сделано суммарно.
    pub reconnect_attempts: u32,
    /// Транспорт восстановлен, но продолжить партию не удалось (например,
    /// исчерпаны попытки/реконнект выключен). `false` после успешного
    /// resume-54 — прогон продолжился.
    pub reconnect_pending_state: bool,
    /// `data_hash`, с которым сервер принял клиента (hex; 64 символа — сервер
    /// без `PATCHES.RES`, 128 — полный).
    pub data_hash: String,
    /// Сколько попыток первого подключения (`Create game → Join → start`)
    /// понадобилось; `1` — сессия уже была готова.
    pub join_attempts: u32,
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
            Some(SessionEvent::Disconnected) => {
                tracing::warn!("reconnect: сервер разорвал ENet-сессию в лобби");
                match reconnect_session(handle, session_config, options.reconnect).await {
                    ReconnectOutcome::Restored { attempts, .. } => bail!(
                        "reconnect: транспорт восстановлен (ENet+TP00, попыток {attempts}), \
                         но повторный вход в лобби невозможен: сервер уже не в Remote_GameState=1 \
                         (docs/protocol.md §9)"
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
    //
    // Без явного `--handshake-patches` пробуем два варианта (полный → только
    // MAX.RES): так Create game работает против сервера без PATCHES.RES без
    // обходного флага.
    let hashes = handshake_data_hashes(&config)?;
    let (joined, session_config, mut handle, data_hash, join_attempts) =
        open_network_session(&config, options, &hashes, &mut sink).await?;

    let lobby = run_lobby(app, &mut handle, &session_config, &config, options).await?;

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
    let level =
        |raw: i32| ResourceLevel::new(raw.clamp(0, 2) as u8).unwrap_or(ResourceLevel::NORMAL);

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
        clans: [max_core::enums::TeamClan::Random; PLAYER_TEAM_COUNT],
        player_team: local_team,
        start_gold: settings.ini_start_gold.max(0) as u32,
        // Сетевая игра стартует без фаз мобилизации/высадки — точки приходят
        // из пакета 30 (`supplies`), состав — стандартный.
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

/// `Remote_SetupPlayers` (`remote.cpp:705-729`): тип команды по её ноде.
///
/// `team_nodes[team] > 4` — слот занят клиентом; своя нода → `Player`, чужая →
/// `Remote`. Незанятый слот остаётся `None` (AI-команды сервера клиент не
/// симулирует и в барьерах не ждёт).
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
    /// Разрывов ENet-сессии за прогон.
    disconnects: u32,
    /// Сделанных попыток переподключения.
    reconnect_attempts: u32,
    /// Успешных восстановлений транспорта и продолжений партии.
    reconnects: u32,
    /// Транспорт восстановлен, но состояние партии не удалось восстановить
    /// (сейчас `false` после успешного resume-54; поле оставлено для отчёта).
    reconnect_pending_state: bool,
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

    /// Состояние своей команды для восстановления после реконнекта.
    ///
    /// Пакет 54 — единственный «снимок» в протоколе (`docs/protocol.md` §8.5,
    /// §9): он несёт юнитов, золото, исследования и счётчики id одной
    /// команды. После разрыва клиент повторно публикует **своё** состояние с
    /// последним завершённым `sync_index`: сервер и пиры применяют его как
    /// авторитетное (`Remote_ReceiveNetPacket_54`), а встречные 54 пиров
    /// корректируют удалённые команды у нас. Полной карты/SNAP в протоколе
    /// нет — см. §9 protocol.md о пределах.
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
    session_config: &SessionConfig,
    stats: &mut GameLoopStats,
    fsm: &mut TurnBarrierFsm,
    options: &NetworkOptions,
) -> Result<bool> {
    loop {
        match handle.next_event_timeout(options.event_wait).await {
            // Разрыв не завершает прогон молча: сначала автоматический
            // ENet connect + TP 00 по ReconnectPolicy, затем resume-54 и
            // продолжение барьеров.
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

                        // Состояние не сбрасываем: lockstep-барьеры не дают
                        // партии уйти вперёд без нашей команды, поэтому
                        // локальный turn/units — точка продолжения. Свой 54
                        // публикуем заново (сервер и пиры применяют состояние
                        // владельца), встречные 54 исправят удалённые команды
                        // на ближайшем SYNC; 53/52/45 догоняются повторами.
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
                // Продолжаем вычерпывать события уже из новой сессии; барьер
                // в wait_barrier повторит свой пакет, как только истечёт
                // трёхсекундный интервал (для него время идёт с прошлой
                // отправки).
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

/// Игровой цикл lockstep-клиента по FSM `docs/barrier-flow.md` §3.
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

    // BEGIN (00): загрузка партии и каждый ход в SimultaneousMoves.
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
        // PLAY (53): итоги баз на начало хода, затем один тик и 06.
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

        // Тики хода. Headless (без окна или без `--turn-wait`) — один тик и
        // сразу 06: ход закрывается, как только разобрана очередь команд.
        // С окном и `--turn-wait N` — интерактивное окно: крутим тики и кадры,
        // пока игрок не нажмёт End Turn (или не истечёт N секунд), иначе
        // человек не успевает ничего сделать — ход длится один кадр.
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

            // Приказы, собранные вводом в этом кадре (38 — движение, 08 — атака):
            // локальное применение + multicast в сессию (M4.3).
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

        // 06 — ровно один раз за ход, когда локальная команда закончила.
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

        // SYNC (54): авторитетное состояние своей команды.
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

        // CRC (45): свой CRC после 54-коррекции, затем сверка с пирами.
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
    use max_core::enums::{TeamClan, UnitType};
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
        assert_eq!(
            planet_from_settings(&settings),
            None,
            "неизвестное имя → None"
        );
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
            app.state().frame_sync_counter[TeamId::GREEN.0 as usize],
            1,
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

    /// Состояние для трёхклиентного сценария: одинаковый мир, по танку на
    /// команду. Клиенты получают клон и меняют только `TeamKind`.
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

    /// Релей в «звезде»: пакет отправителя получают все остальные клиенты.
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

    /// P5: три клиента, разрыв B, восстановление через 54 и продолжение хода.
    ///
    /// Мок-транспорт — детерминированный релей без сети: пока B отключён, его
    /// пакеты не доставляются; A/C успевают «сходить» и испортить копию юнита
    /// B (как пропущенный приказ). После реконнекта B публикует свой 54 —
    /// A/C принимают состояние владельца; встречный 54 A исправляет копию
    /// юнита A у B. Ход доходит до 45, CRC и turn_counter совпадают.
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

        // 00: все трое.
        send_all(&mut clients, &mut stats, |client| {
            client.fsm.begin(client.app.state_mut())
        });
        assert!(clients.iter().all(|client| client
            .fsm
            .reached(client.app.state())), "00 с 3 участниками");

        // 53: все трое.
        send_all(&mut clients, &mut stats, |client| {
            client.fsm.complex(client.app.state())
        });
        assert!(clients.iter().all(|client| client
            .fsm
            .reached(client.app.state())), "53 с 3 участниками");

        // 06: локальный конец хода, релей остальным.
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

        // B (GREEN) пропадает: A и C шлют 52, но B не получает их, а его 52
        // теряется. Без B барьер не закрывается — это и есть lockstep.
        let mut packets: Vec<Vec<Packet>> = Vec::new();
        for client in &mut clients {
            packets.push(client.fsm.end(client.app.state_mut()));
        }
        relay(&packets, &[0, 2], &mut clients, &mut stats);
        assert!(!clients[0].fsm.reached(clients[0].app.state()));
        assert!(!clients[2].fsm.reached(clients[2].app.state()));

        // Пока B нет, A/C меняют копию танка B (пропущенный приказ атаки).
        for index in [0usize, 2] {
            clients[index]
                .app
                .state_mut()
                .unit_mut(ids[1])
                .expect("tank B")
                .hits = 1;
        }
        // И у B устарела копия танка A.
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

        // Реконнект: B публикует свой 54 (sync_index ещё 0), A/C применяют.
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

        // B повторяет свой потерянный 52 (тот же счётчик, без нового
        // pre-increment); A/C уже ждут именно его.
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

        // 54 всех команд: B получает актуальное состояние A/C, A/C — B.
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

        // 45: CRC каждого клиента сходится со всеми.
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

    /// Resume-54 после реконнекта: своя команда, последний sync_index и полный
    /// набор юнитов команды (включая хвост экономики в last-чанке).
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
        assert_eq!(
            apps[0].state().frame_sync_counter[TeamId::GREEN.0 as usize],
            1
        );
        assert_eq!(
            apps[1].state().frame_sync_counter[TeamId::RED.0 as usize],
            1
        );
    }

    /// Разрыв в обработчике событий сам по себе не паникует и завершает
    /// прогон (retry выполняет `pump_events`, здесь проверяется fallback).
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

        // Новый разрыв — новый раунд, ошибка прошлого не тащится.
        tracker.note_disconnect();
        assert_eq!(tracker.attempts(), 0);
        assert!(tracker.last_error().is_none());
        assert_eq!(tracker.next_attempt(), Some(1));
    }

    /// Мок-сессия: первая попытка падает, вторая проходит — retry-логика
    /// восстанавливает транспорт и сообщает номер успешной попытки.
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

    /// Все попытки неудачны: `Exhausted` с числом попыток, без паник.
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

    /// `attempts = 0` — реконнект выключен, коннектор не вызывается.
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

    /// Таймаут попытки не вешает прогон и попадает в `last_error`.
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
    // M-NET-доводки: ретрай первого подключения и fallback data_hash
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

    /// Мок-коннектор: первая попытка падает, вторая проходит. Время
    /// контролируется `start_paused`: между попытками ровно backoff (1 с).
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

    /// Таймаут попытки ограничен политикой и не вешает прогон: 2 с + 1 с
    /// backoff + 2 с = 5 с виртуального времени на две попытки.
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

    /// `TP 03 GameData` → один переподключ с хэшем только MAX.RES (тикет тот
    /// же), сессия стартует. Отвергается именно полный хэш.
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

    /// Отказ по другой причине (`Version`) — без fallback и без ретрая.
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

    /// Сетевой цикл ждёт `PATCHES.RES`-вариант только без явного флага.
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

        // Явный --handshake-patches — ровно один вариант, без угадывания.
        let explicit = AppConfig {
            handshake_patches: Some(dir.join("nonexistent.res")),
            ..config.clone()
        };
        let explicit_hashes = handshake_data_hashes(&explicit).expect("хэши");
        assert_eq!(explicit_hashes.len(), 1);
        assert!(explicit_hashes[0].patches_res.is_none());

        // Локального PATCHES.RES нет — полный вариант уже MAX-only.
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
