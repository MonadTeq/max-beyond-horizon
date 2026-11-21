//! The gRPC adapter's backend: the same lobby/admin logic as the `max_lobby` /
//! `max_admin` WebSocket plugins, expressed as typed operations.
//!
//! [`HorizonBackend`] is the seam between the gRPC transport and the session
//! service. [`MaxBackend`] is the production implementation: it owns a
//! [`max_session::SessionManager`] (created exactly like the plugins create
//! theirs, from the `[max]` config section plus `MAX_*` environment variables)
//! and reproduces the plugins' semantics field by field:
//!
//! * the lobby registry counts the creator as a player, drops entries the
//!   manager no longer tracks, and keeps only a non-cryptographic hash of a
//!   protected game's password;
//! * `join_game` only succeeds for a `Running` session and mints a signed
//!   per-player ticket through [`max_auth`] when `MAX_SESSION_SECRET` is set,
//!   otherwise it returns the legacy password-as-ticket;
//! * admin authorization is the fail-closed HMAC token of `max_admin`;
//! * failures carry the exact legacy WS text, so they map 1:1 onto gRPC statuses
//!   ([`crate::error`]).
//!
//! The session manager is abstracted by [`SessionControl`] so tests can run the
//! full lobby/admin logic over a fake session service (see [`crate::testing`]),
//! without spawning `max-port` processes.
//!
//! ## Relation to the WebSocket plugins
//!
//! `max-brh-docs/protocol-grpc.md` §4.4 notes that the plugins and this adapter each
//! hold a `SessionManager`: sharing one handle across a `cdylib` plugin and the
//! host binary is a separate refactor of the plugin loader, so phase 1 keeps
//! *separate* managers (the WS server is untouched). Each manager is
//! configured from the same `[max]` section, so games, tickets and ports behave
//! identically on both transports.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::RangeInclusive;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use max_auth::Auth;
use max_session::{
    load_max_toml_section, MaxConfig, SessionConfig, SessionInfo, SessionManager, SessionStatus,
};
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use crate::admin::verify_admin_token;
use crate::error::ServiceError;

/// Lifetime of a signed per-player join ticket (`max-brh-docs/protocol.md` §6: 24 h).
pub const TICKET_TTL: Duration = Duration::from_secs(24 * 60 * 60);

// ============================================================================
// Domain types (mirror the proto schema, but transport-independent)
// ============================================================================

/// Session lifecycle state (`GameStatus` in `types.proto`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameStatus {
    /// The headless process is being spawned (or a restart is pending).
    Starting,
    /// The process is running and accepting players.
    Running,
    /// The process exited on its own.
    Exited,
    /// The session was stopped through the manager.
    Stopped,
    /// The id is unknown to the manager (admin status only).
    Unknown,
}

impl GameStatus {
    /// Maps a [`max_session::SessionStatus`].
    pub fn from_session_status(status: &SessionStatus) -> Self {
        match status {
            SessionStatus::Starting => Self::Starting,
            SessionStatus::Running => Self::Running,
            SessionStatus::Exited(_) => Self::Exited,
            SessionStatus::Stopped => Self::Stopped,
        }
    }

    /// Legacy WS `status`/`state` string (`max-brh-docs/protocol.md` §5.3).
    pub fn as_ws_str(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Exited => "exited",
            Self::Stopped => "stopped",
            Self::Unknown => "unknown",
        }
    }
}

/// One lobby game (`GameInfo` in `types.proto`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInfo {
    /// Session UUID.
    pub id: Uuid,
    /// Human-readable game name.
    pub name: String,
    /// World index (`0..=23`).
    pub world: i64,
    /// Lifecycle state.
    pub status: GameStatus,
    /// UDP port of the headless process.
    pub port: u16,
    /// Players registered in the lobby, counting the creator.
    pub players: u32,
    /// Minimum players before the headless server starts.
    pub min_players: u32,
    /// Whether the game is password protected.
    pub password_protected: bool,
}

/// Ticket format (`TicketKind` in `types.proto`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketKind {
    /// Password-as-ticket (`""` for open games).
    Legacy,
    /// `v1.<session_id>.<player_id>.<unix_expiry>.<hex_hmac>`.
    Signed,
}

/// A session ticket handed to the ENet `TP 00` handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketInfo {
    /// Exact string the client sends in the handshake.
    pub value: String,
    /// Which format [`Self::value`] carries.
    pub kind: TicketKind,
    /// Parsed session id of a signed ticket.
    pub session_id: Option<Uuid>,
    /// Parsed player id of a signed ticket.
    pub player_id: Option<Uuid>,
    /// Expiration of a signed ticket in whole Unix seconds.
    pub expires_at_unix: Option<i64>,
}

/// Result of `CreateGame`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedGame {
    /// The created lobby entry.
    pub game: GameInfo,
    /// Ticket handed to the creator.
    pub ticket: TicketInfo,
}

/// Result of `JoinGame`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinedGame {
    /// Session UUID (echo of the request).
    pub game_id: Uuid,
    /// Host the ENet client connects to (`[server.grpc].announce_host`, default
    /// `127.0.0.1`, WS parity).
    pub host: String,
    /// UDP port of the headless process.
    pub port: u16,
    /// Ticket to present in `TP 00`.
    pub ticket: TicketInfo,
}

/// One admin session row (`AdminSession` in `types.proto`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminSession {
    /// Session UUID.
    pub game_id: Uuid,
    /// Human-readable game name.
    pub name: String,
    /// UDP port of the headless process.
    pub port: u16,
    /// Lifecycle state (`state` in the WS frame).
    pub state: GameStatus,
    /// Session directory on the server filesystem.
    pub dir: String,
}

/// The optional game fields shared by `create_game` and `start_session`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameOptions {
    /// Human-readable game name.
    pub name: Option<String>,
    /// World index.
    pub world: Option<i64>,
    /// Seconds per turn.
    pub timer: Option<i64>,
    /// End-turn timer in seconds.
    pub endturn: Option<i64>,
    /// 1 = turn based, 0 = simultaneous.
    pub play_mode: Option<i64>,
    /// Starting credits.
    pub start_gold: Option<i64>,
    /// Victory type.
    pub victory_type: Option<i64>,
    /// Victory limit.
    pub victory_limit: Option<i64>,
    /// Minimum players.
    pub min_players: Option<u32>,
}

impl GameOptions {
    /// Applies the set fields onto a [`SessionConfig`], leaving the rest at their
    /// defaults (the contract's "unset fields keep the defaults" rule).
    fn apply(&self, config: &mut SessionConfig) {
        if let Some(name) = &self.name {
            config.name = Some(name.clone());
        }
        if let Some(value) = self.world {
            config.world = value;
        }
        if let Some(value) = self.timer {
            config.timer = value;
        }
        if let Some(value) = self.endturn {
            config.endturn = value;
        }
        if let Some(value) = self.play_mode {
            config.play_mode = value;
        }
        if let Some(value) = self.start_gold {
            config.start_gold = value;
        }
        if let Some(value) = self.victory_type {
            config.victory_type = value;
        }
        if let Some(value) = self.victory_limit {
            config.victory_limit = value;
        }
        if let Some(value) = self.min_players {
            config.min_players = value;
        }
    }
}

/// Parameters of `CreateGame` (all optional).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreateGameParams {
    /// Game rules and name.
    pub game: GameOptions,
    /// Password; when set the game is protected and the password is the ticket.
    pub password: Option<String>,
}

/// Parameters of `AdminSessionStart`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminStartParams {
    /// **Conflict pre-check only**: the manager always allocates a port itself.
    pub port: Option<u32>,
    /// Game rules and name.
    pub game: GameOptions,
}

/// Outcome of `AdminSessionStop` (`ok` + inline error, WS parity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopOutcome {
    /// Session id (echo of the request).
    pub game_id: String,
    /// True when the session was stopped.
    pub ok: bool,
    /// Inline error when `ok == false`.
    pub error: Option<ServiceError>,
}

/// Outcome of `AdminSessionRestart` (`ok` + inline error, WS parity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartOutcome {
    /// Id of the stopped session (echo of the request).
    pub game_id: String,
    /// True when the old session was stopped and a fresh one started.
    pub ok: bool,
    /// Id of the fresh session on success.
    pub new_game_id: Option<Uuid>,
    /// Port of the fresh session on success.
    pub port: Option<u16>,
    /// Inline error when `ok == false`.
    pub error: Option<ServiceError>,
}

/// Outcome of `AdminSessionStart` (`ok` + inline error, WS parity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartOutcome {
    /// Id of the started session on success.
    pub game_id: Option<Uuid>,
    /// Port of the started session on success.
    pub port: Option<u16>,
    /// True on success.
    pub ok: bool,
    /// Inline error when `ok == false`.
    pub error: Option<ServiceError>,
}

// ============================================================================
// Session service seam (the "existing service" the adapter is thin over)
// ============================================================================

/// The session lifecycle operations the lobby/admin logic needs.
///
/// The production implementation is [`SessionManagerControl`] over
/// [`max_session::SessionManager`]; tests use [`crate::testing::MockSessions`].
/// Errors are plain strings so they stay verbatim the messages the WS plugins
/// forward in their `error` fields.
#[async_trait]
pub trait SessionControl: Send + Sync {
    /// Starts a session (allocates a port, prepares its directory, spawns it).
    async fn start(&self, config: SessionConfig) -> Result<SessionInfo, String>;
    /// Current status of a session, or `None` when the manager does not track it.
    fn health(&self, id: Uuid) -> Option<SessionStatus>;
    /// Snapshot of every tracked session.
    fn list(&self) -> Vec<SessionInfo>;
    /// Stops a session and releases its port.
    async fn stop(&self, id: Uuid) -> Result<(), String>;
    /// Stops a session and starts a fresh one with the same settings.
    async fn restart(&self, id: Uuid) -> Result<SessionInfo, String>;
    /// Stops every tracked session (graceful shutdown).
    async fn shutdown_all(&self);
}

/// [`SessionControl`] implementation over the real [`max_session::SessionManager`].
pub struct SessionManagerControl {
    manager: Arc<SessionManager>,
}

impl SessionManagerControl {
    /// Wraps a shared manager.
    pub fn new(manager: Arc<SessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl SessionControl for SessionManagerControl {
    async fn start(&self, config: SessionConfig) -> Result<SessionInfo, String> {
        self.manager.start(config).await.map_err(|error| error.to_string())
    }

    fn health(&self, id: Uuid) -> Option<SessionStatus> {
        self.manager.health(id)
    }

    fn list(&self) -> Vec<SessionInfo> {
        self.manager.list()
    }

    async fn stop(&self, id: Uuid) -> Result<(), String> {
        self.manager.stop(id).await.map_err(|error| error.to_string())
    }

    async fn restart(&self, id: Uuid) -> Result<SessionInfo, String> {
        self.manager.restart(id).await.map_err(|error| error.to_string())
    }

    async fn shutdown_all(&self) {
        self.manager.shutdown_all().await;
    }
}

// ============================================================================
// Backend trait
// ============================================================================

/// Operations behind the ten `horizon.v1.Horizon` RPCs.
///
/// Every method returns the same data/errors the corresponding WS handler
/// produces; the gRPC layer only converts to protobuf (see [`crate::service`]).
#[async_trait]
pub trait HorizonBackend: Send + Sync {
    /// Lobby snapshot (`max_lobby:list_games`).
    async fn list_games(&self) -> Result<Vec<GameInfo>, ServiceError>;

    /// Starts a session and returns its lobby entry + creator ticket
    /// (`max_lobby:create_game`).
    async fn create_game(
        &self,
        player_id: Uuid,
        params: CreateGameParams,
    ) -> Result<CreatedGame, ServiceError>;

    /// Validates a game and returns host/port/ticket (`max_lobby:join_game`).
    async fn join_game(
        &self,
        player_id: Uuid,
        game_id: &str,
        password: Option<String>,
    ) -> Result<JoinedGame, ServiceError>;

    /// Admin snapshot (`max_admin:list_sessions`).
    async fn admin_list_sessions(
        &self,
        admin_token: Option<String>,
    ) -> Result<Vec<AdminSession>, ServiceError>;

    /// Admin lifecycle state of one session (`max_admin:session_status`).
    async fn admin_session_status(
        &self,
        admin_token: Option<String>,
        game_id: &str,
    ) -> Result<GameStatus, ServiceError>;

    /// Stops a session (`max_admin:stop_session`).
    async fn admin_session_stop(
        &self,
        admin_token: Option<String>,
        game_id: &str,
    ) -> Result<StopOutcome, ServiceError>;

    /// Restarts a session (`max_admin:restart_session`).
    async fn admin_session_restart(
        &self,
        admin_token: Option<String>,
        game_id: &str,
    ) -> Result<RestartOutcome, ServiceError>;

    /// Starts a session (`max_admin:start_session`).
    async fn admin_session_start(
        &self,
        admin_token: Option<String>,
        params: AdminStartParams,
    ) -> Result<StartOutcome, ServiceError>;

    /// Stops every session this backend owns (called when the listener stops).
    async fn shutdown(&self) {}
}

// ============================================================================
// Production backend over max_session / max_auth
// ============================================================================

/// Lobby registry entry, mirroring `max_lobby::LobbyGame`.
///
/// Passwords are never stored: for protected games only a non-cryptographic
/// hash of the ticket (= password) is kept, exactly as the WS plugin does.
struct LobbyGame {
    info: SessionInfo,
    world: i64,
    min_players: u32,
    password_protected: bool,
    #[allow(dead_code)] // parity with the WS registry; useful in debug logs
    created_by: Uuid,
    players: Vec<Uuid>,
    ticket_hash: Option<u64>,
}

/// The production [`HorizonBackend`]: [`SessionManager`] + `max_auth` + the
/// lobby registry + the admin token check.
pub struct MaxBackend {
    sessions: Option<Arc<dyn SessionControl>>,
    games: Mutex<HashMap<Uuid, LobbyGame>>,
    /// HMAC secret for signed join tickets; `None`/empty keeps the legacy format.
    session_secret: Option<String>,
    /// HMAC secret for admin tokens; `None` fails every admin RPC closed.
    admin_secret: Option<String>,
    /// Inclusive UDP port range sessions are allocated from (`[max] port_range_*`).
    port_range: RangeInclusive<u16>,
    /// Advertised host of `JoinGameResponse.host`; comes from
    /// `[server.grpc].announce_host`, defaulting to `127.0.0.1` (WS parity)
    /// (`max-brh-docs/protocol-grpc.md` §4.6 keeps the field for exactly this).
    host: String,
}

impl MaxBackend {
    /// Builds a backend from an explicit session service (tests, or a caller
    /// that already owns a manager).
    pub fn new(
        sessions: Option<Arc<dyn SessionControl>>,
        session_secret: Option<String>,
        admin_secret: Option<String>,
        port_range: RangeInclusive<u16>,
    ) -> Self {
        Self {
            sessions,
            games: Mutex::new(HashMap::new()),
            session_secret,
            admin_secret,
            port_range,
            host: "127.0.0.1".to_string(),
        }
    }

    /// Loads the `[max]` section from `config_path` plus the `MAX_*` environment
    /// variables and creates the session manager, exactly like `max_lobby` and
    /// `max_admin` do in `on_init`.
    ///
    /// A missing/invalid `[max]` section or unreadable `config.toml` is not
    /// fatal: the backend stays up and every lobby/admin RPC answers the same
    /// "is not configured" error the WS plugins send.
    pub fn from_config_file(config_path: &Path) -> Self {
        let section = match load_max_toml_section(config_path) {
            Ok(section) => Some(section),
            Err(error) => {
                warn!(
                    %error,
                    path = %config_path.display(),
                    "horizon_grpc: ignoring the [max] config section; using MAX_* environment variables only"
                );
                None
            }
        };

        let config = match MaxConfig::from_env_and_toml(section.as_ref()) {
            Ok(config) => config,
            Err(error) => {
                warn!(
                    %error,
                    "horizon_grpc: invalid [max] configuration; using MAX_* environment variables only"
                );
                MaxConfig::from_env_and_toml(None)
                    .expect("environment-only configuration loading cannot fail")
            }
        };

        Self::from_max_config(config)
    }

    /// Builds the backend from an already loaded [`MaxConfig`].
    pub fn from_max_config(config: MaxConfig) -> Self {
        let session_secret = config.session_secret.clone();
        let admin_secret = config.admin_secret.clone();
        let port_range = config.port_range.clone();

        // Same rule as the plugins: both executable and template_dir are
        // required, otherwise the lobby stays "not configured".
        let configured = !config.executable.as_os_str().is_empty()
            && !config.template_dir.as_os_str().is_empty();

        let sessions: Option<Arc<dyn SessionControl>> = if configured {
            match SessionManager::new(config) {
                Ok(manager) => {
                    info!("horizon_grpc: session manager created; lobby/admin RPCs are ready");
                    Some(Arc::new(SessionManagerControl::new(manager)) as Arc<dyn SessionControl>)
                }
                Err(error) => {
                    warn!(
                        %error,
                        "horizon_grpc: session manager rejected the configuration; lobby/admin RPCs will report it is not configured"
                    );
                    None
                }
            }
        } else {
            warn!(
                "horizon_grpc: executable / template_dir are not set (MAX_SERVER_EXE / MAX_TEMPLATE_DIR or the [max] config section); lobby/admin RPCs will report it is not configured"
            );
            None
        };

        Self::new(sessions, session_secret, admin_secret, port_range)
    }

    /// Overrides the advertised host of `JoinGameResponse.host`.
    ///
    /// `serve` applies `[server.grpc].announce_host` here; the default is
    /// `127.0.0.1`, the legacy WS value.
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// The session service, or the error the WS plugins send when unconfigured.
    fn sessions_or_lobby_error(&self) -> Result<Arc<dyn SessionControl>, ServiceError> {
        self.sessions
            .clone()
            .ok_or_else(|| ServiceError::unavailable("max_lobby is not configured"))
    }

    /// The session service, or the admin-side "not configured" error.
    fn sessions_or_admin_error(&self) -> Result<Arc<dyn SessionControl>, ServiceError> {
        self.sessions
            .clone()
            .ok_or_else(|| ServiceError::unavailable("max_admin is not configured"))
    }

    /// Fail-closed admin authorization (`max-brh-docs/protocol.md` §5.4), with the exact
    /// legacy `admin_error` texts.
    fn authorize(&self, admin_token: Option<&str>) -> Result<(), ServiceError> {
        let Some(secret) = self.admin_secret.as_deref() else {
            return Err(ServiceError::unauthenticated(
                "admin authorization is disabled: no admin_secret is configured",
            ));
        };
        let Some(token) = admin_token else {
            return Err(ServiceError::unauthenticated("missing admin_token"));
        };
        if verify_admin_token(secret, token) {
            Ok(())
        } else {
            Err(ServiceError::unauthenticated("invalid admin_token"))
        }
    }

    /// The ticket handed to a joining player (`max_lobby::join_ticket`).
    fn join_ticket(&self, game_id: Uuid, player_id: Uuid, fallback: String) -> String {
        match self.session_secret.as_deref().filter(|secret| !secret.is_empty()) {
            Some(secret) => Auth::new(secret).issue(game_id, player_id, TICKET_TTL),
            None => fallback,
        }
    }
}

#[async_trait]
impl HorizonBackend for MaxBackend {
    async fn list_games(&self) -> Result<Vec<GameInfo>, ServiceError> {
        let sessions = self.sessions_or_lobby_error()?;
        let mut lobby = self.games.lock().await;

        // Drop entries the manager no longer tracks (stopped, or the watchdog
        // gave up on restarts), exactly like max_lobby::handle_list_games.
        let gone: Vec<Uuid> = lobby
            .iter()
            .filter(|(id, _)| sessions.health(**id).is_none())
            .map(|(id, _)| *id)
            .collect();
        for id in &gone {
            lobby.remove(id);
        }

        let games = lobby
            .iter()
            .map(|(id, game)| GameInfo {
                id: *id,
                name: game.info.name.clone(),
                world: game.world,
                status: sessions
                    .health(*id)
                    .map(|status| GameStatus::from_session_status(&status))
                    .unwrap_or(GameStatus::Stopped),
                port: game.info.port,
                players: game.players.len() as u32,
                min_players: game.min_players,
                password_protected: game.password_protected,
            })
            .collect();

        Ok(games)
    }

    async fn create_game(
        &self,
        player_id: Uuid,
        params: CreateGameParams,
    ) -> Result<CreatedGame, ServiceError> {
        let sessions = self.sessions_or_lobby_error()?;
        let mut lobby = self.games.lock().await;

        let mut config = SessionConfig::default();
        params.game.apply(&mut config);

        // A network game needs at least two human players (WS text verbatim).
        if config.min_players < 2 {
            return Err(ServiceError::invalid_argument(
                "a network game needs at least 2 players",
            ));
        }

        // The password doubles as the session ticket for protected games.
        let password_protected = params.password.is_some();
        config.password_protected = password_protected;
        if let Some(password) = &params.password {
            config.ticket = Some(password.clone());
        }

        let (world, min_players) = (config.world, config.min_players);
        let info = sessions
            .start(config)
            .await
            .map_err(ServiceError::from_ws)?;

        let ticket = params.password.clone().unwrap_or_default();
        lobby.insert(
            info.id,
            LobbyGame {
                info: info.clone(),
                world,
                min_players,
                password_protected,
                created_by: player_id,
                // The creator is a player of their own game.
                players: vec![player_id],
                ticket_hash: password_protected.then(|| ticket_hash(&ticket)),
            },
        );

        let game = lobby.get(&info.id).expect("game just inserted");
        let status = sessions
            .health(info.id)
            .map(|status| GameStatus::from_session_status(&status))
            .unwrap_or(GameStatus::Starting);

        Ok(CreatedGame {
            game: GameInfo {
                id: info.id,
                name: game.info.name.clone(),
                world: game.world,
                status,
                port: game.info.port,
                players: game.players.len() as u32,
                min_players: game.min_players,
                password_protected: game.password_protected,
            },
            ticket: TicketInfo {
                value: ticket,
                kind: TicketKind::Legacy,
                session_id: None,
                player_id: None,
                expires_at_unix: None,
            },
        })
    }

    async fn join_game(
        &self,
        player_id: Uuid,
        game_id: &str,
        password: Option<String>,
    ) -> Result<JoinedGame, ServiceError> {
        let sessions = self.sessions_or_lobby_error()?;
        let mut lobby = self.games.lock().await;

        let id = match Uuid::parse_str(game_id) {
            Ok(id) => id,
            // "Not a UUID" is indistinguishable from "unknown game" for the WS
            // client and both are NOT_FOUND (max-brh-docs/protocol-grpc.md §6.1).
            Err(_) => return Err(ServiceError::not_found("game not found")),
        };

        let Some(game) = lobby.get(&id) else {
            return Err(ServiceError::not_found("game not found"));
        };

        // Protected games are joined by comparing hashes; the plaintext password
        // is never stored in the registry.
        if game.password_protected {
            let matches = match (&password, game.ticket_hash) {
                (Some(password), Some(expected)) => ticket_hash(password) == expected,
                _ => false,
            };
            if !matches {
                return Err(ServiceError::permission_denied("invalid password"));
            }
        }

        match sessions.health(id) {
            Some(SessionStatus::Running) => {}
            Some(status) => {
                return Err(ServiceError::failed_precondition(format!(
                    "game is not running (status: {})",
                    GameStatus::from_session_status(&status).as_ws_str()
                )));
            }
            None => {
                return Err(ServiceError::failed_precondition("game is not running"));
            }
        }

        let port = game.info.port;
        let legacy_ticket = if game.password_protected {
            password.clone().unwrap_or_default()
        } else {
            String::new()
        };
        let ticket = self.join_ticket(id, player_id, legacy_ticket);
        let ticket = ticket_info(ticket, id, player_id);

        if let Some(game) = lobby.get_mut(&id) {
            if !game.players.contains(&player_id) {
                game.players.push(player_id);
            }
        }

        Ok(JoinedGame {
            game_id: id,
            host: self.host.clone(),
            port,
            ticket,
        })
    }

    async fn admin_list_sessions(
        &self,
        admin_token: Option<String>,
    ) -> Result<Vec<AdminSession>, ServiceError> {
        self.authorize(admin_token.as_deref())?;
        let sessions = self.sessions_or_admin_error()?;

        Ok(sessions
            .list()
            .into_iter()
            .map(|info| AdminSession {
                game_id: info.id,
                name: info.name,
                port: info.port,
                state: GameStatus::from_session_status(&info.status),
                dir: info.dir.to_string_lossy().into_owned(),
            })
            .collect())
    }

    async fn admin_session_status(
        &self,
        admin_token: Option<String>,
        game_id: &str,
    ) -> Result<GameStatus, ServiceError> {
        self.authorize(admin_token.as_deref())?;
        let id = parse_admin_game_id(game_id)?;
        let sessions = self.sessions_or_admin_error()?;

        Ok(sessions
            .health(id)
            .map(|status| GameStatus::from_session_status(&status))
            // Unknown session ids answer "unknown" in the WS protocol.
            .unwrap_or(GameStatus::Unknown))
    }

    async fn admin_session_stop(
        &self,
        admin_token: Option<String>,
        game_id: &str,
    ) -> Result<StopOutcome, ServiceError> {
        self.authorize(admin_token.as_deref())?;
        let id = parse_admin_game_id(game_id)?;
        let sessions = self.sessions_or_admin_error()?;

        match sessions.stop(id).await {
            Ok(()) => {
                info!(session = %id, "horizon_grpc: session stopped");
                Ok(StopOutcome {
                    game_id: game_id.to_string(),
                    ok: true,
                    error: None,
                })
            }
            Err(message) => {
                warn!(session = %id, %message, "horizon_grpc: failed to stop session");
                Ok(StopOutcome {
                    game_id: game_id.to_string(),
                    ok: false,
                    error: Some(ServiceError::from_ws(message)),
                })
            }
        }
    }

    async fn admin_session_restart(
        &self,
        admin_token: Option<String>,
        game_id: &str,
    ) -> Result<RestartOutcome, ServiceError> {
        self.authorize(admin_token.as_deref())?;
        let id = parse_admin_game_id(game_id)?;
        let sessions = self.sessions_or_admin_error()?;

        match sessions.restart(id).await {
            Ok(info) => {
                info!(
                    session = %id,
                    new_session = %info.id,
                    port = info.port,
                    "horizon_grpc: session restarted"
                );
                Ok(RestartOutcome {
                    game_id: game_id.to_string(),
                    ok: true,
                    new_game_id: Some(info.id),
                    port: Some(info.port),
                    error: None,
                })
            }
            Err(message) => {
                warn!(session = %id, %message, "horizon_grpc: failed to restart session");
                Ok(RestartOutcome {
                    game_id: game_id.to_string(),
                    ok: false,
                    new_game_id: None,
                    port: None,
                    error: Some(ServiceError::from_ws(message)),
                })
            }
        }
    }

    async fn admin_session_start(
        &self,
        admin_token: Option<String>,
        params: AdminStartParams,
    ) -> Result<StartOutcome, ServiceError> {
        self.authorize(admin_token.as_deref())?;
        let sessions = self.sessions_or_admin_error()?;

        // Optional port *pre-check* only: the manager always allocates ports
        // itself, so a requested port is validated against the configured range
        // and the running sessions but the session still receives an allocated
        // port (returned in the response).
        if let Some(port) = params.port {
            // A value above u16::MAX cannot be inside a u16 port range.
            let in_range = u16::try_from(port).is_ok_and(|port| self.port_range.contains(&port));
            if !in_range {
                return Err(ServiceError::invalid_argument(format!(
                    "requested port {port} is outside the configured MAX_PORT_RANGE ({}..={})",
                    self.port_range.start(),
                    self.port_range.end()
                )));
            }
            if let Some(existing) = sessions.list().iter().find(|info| u32::from(info.port) == port) {
                // WS parity: the collision is an inline `session_started` failure.
                return Ok(StartOutcome {
                    game_id: None,
                    port: None,
                    ok: false,
                    error: Some(ServiceError::new(
                        tonic::Code::AlreadyExists,
                        format!("port {port} is already used by session {}", existing.id),
                    )),
                });
            }
        }

        let mut config = SessionConfig::default();
        params.game.apply(&mut config);

        match sessions.start(config).await {
            Ok(info) => {
                info!(session = %info.id, port = info.port, "horizon_grpc: session started");
                Ok(StartOutcome {
                    game_id: Some(info.id),
                    port: Some(info.port),
                    ok: true,
                    error: None,
                })
            }
            Err(message) => {
                warn!(%message, "horizon_grpc: failed to start a session");
                Ok(StartOutcome {
                    game_id: None,
                    port: None,
                    ok: false,
                    error: Some(ServiceError::from_ws(message)),
                })
            }
        }
    }

    async fn shutdown(&self) {
        if let Some(sessions) = &self.sessions {
            sessions.shutdown_all().await;
        }
    }
}

/// Parses an admin `game_id` with the exact legacy error text.
fn parse_admin_game_id(game_id: &str) -> Result<Uuid, ServiceError> {
    Uuid::parse_str(game_id).map_err(|_| {
        ServiceError::invalid_argument(format!(
            "invalid game_id {game_id:?}: expected a UUID"
        ))
    })
}

/// Non-cryptographic hash of a session ticket (`max_lobby::ticket_hash`).
fn ticket_hash(ticket: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ticket.hash(&mut hasher);
    hasher.finish()
}

/// Builds the [`TicketInfo`] for a ticket string, parsing signed tickets.
///
/// The value is minted by this backend (or returned verbatim as the legacy
/// password), so only the *shape* is parsed here to fill the convenience fields
/// of `Ticket`; the signature is verified by the game server, which owns the
/// secret. A `v1.*` value that does not parse is reported as a legacy ticket
/// instead of claiming fields it does not have.
fn ticket_info(value: String, game_id: Uuid, player_id: Uuid) -> TicketInfo {
    if value.starts_with("v1.") {
        if let Some(parsed) = parse_signed_ticket(&value) {
            if parsed.0 == game_id && parsed.1 == player_id {
                return TicketInfo {
                    value,
                    kind: TicketKind::Signed,
                    session_id: Some(parsed.0),
                    player_id: Some(parsed.1),
                    expires_at_unix: Some(parsed.2),
                };
            }
        }
    }

    TicketInfo {
        value,
        kind: TicketKind::Legacy,
        session_id: None,
        player_id: None,
        expires_at_unix: None,
    }
}

/// Parses the `session_id.player_id.expiry` body of a `v1.*` ticket.
fn parse_signed_ticket(ticket: &str) -> Option<(Uuid, Uuid, i64)> {
    let (body, _mac) = ticket.rsplit_once('.')?;
    let mut fields = body.split('.');
    if fields.next()? != "v1" {
        return None;
    }
    let session_id = Uuid::parse_str(fields.next()?).ok()?;
    let player_id = Uuid::parse_str(fields.next()?).ok()?;
    let expires_at_unix = fields.next()?.parse::<i64>().ok()?;
    if fields.next().is_some() {
        return None;
    }
    Some((session_id, player_id, expires_at_unix))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MockSessions;

    fn backend(sessions: Arc<MockSessions>, secret: Option<&str>) -> MaxBackend {
        MaxBackend::new(
            Some(sessions as Arc<dyn SessionControl>),
            secret.map(str::to_string),
            Some("admin-test-secret".to_string()),
            32000..=32010,
        )
    }

    fn params(min_players: u32) -> CreateGameParams {
        CreateGameParams {
            game: GameOptions {
                name: Some("Friday 2v2".to_string()),
                world: Some(3),
                min_players: Some(min_players),
                ..GameOptions::default()
            },
            password: None,
        }
    }

    #[tokio::test]
    async fn list_games_is_empty_before_anything_is_created() {
        let backend = backend(MockSessions::new(), None);

        assert!(backend.list_games().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_game_registers_the_creator_and_returns_the_legacy_ticket() {
        let sessions = MockSessions::new();
        let backend = backend(Arc::clone(&sessions), None);

        let created = backend
            .create_game(Uuid::new_v4(), params(4))
            .await
            .unwrap();

        assert_eq!(created.game.name, "Friday 2v2");
        assert_eq!(created.game.world, 3);
        assert_eq!(created.game.min_players, 4);
        assert_eq!(created.game.players, 1);
        assert_eq!(created.game.status, GameStatus::Starting);
        assert_eq!(created.ticket.kind, TicketKind::Legacy);
        assert_eq!(created.ticket.value, "");

        let games = backend.list_games().await.unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].id, created.game.id);
    }

    #[tokio::test]
    async fn create_game_keeps_the_password_as_ticket_but_stores_only_a_hash() {
        let backend = backend(MockSessions::new(), None);
        let created = backend
            .create_game(
                Uuid::new_v4(),
                CreateGameParams {
                    password: Some("sesame".to_string()),
                    ..params(2)
                },
            )
            .await
            .unwrap();

        assert!(created.game.password_protected);
        assert_eq!(created.ticket.value, "sesame");
        assert_eq!(created.ticket.kind, TicketKind::Legacy);
    }

    #[tokio::test]
    async fn create_game_rejects_fewer_than_two_players_with_the_ws_text() {
        let backend = backend(MockSessions::new(), None);

        let error = backend
            .create_game(Uuid::new_v4(), params(1))
            .await
            .unwrap_err();

        assert_eq!(error.code, tonic::Code::InvalidArgument);
        assert_eq!(error.message, "a network game needs at least 2 players");
    }

    #[tokio::test]
    async fn list_games_drops_sessions_the_manager_no_longer_tracks() {
        let sessions = MockSessions::new();
        let backend = backend(Arc::clone(&sessions), None);

        let created = backend
            .create_game(Uuid::new_v4(), params(2))
            .await
            .unwrap();
        assert_eq!(backend.list_games().await.unwrap().len(), 1);

        sessions.stop(created.game.id).await.unwrap();
        assert!(backend.list_games().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn join_game_rejects_unknown_and_malformed_game_ids_as_not_found() {
        let backend = backend(MockSessions::new(), None);

        for game_id in ["5e2f9f3a-8b4d-4c1e-9a7b-2d0e6f8a1c3b", "not-a-uuid"] {
            let error = backend
                .join_game(Uuid::new_v4(), game_id, None)
                .await
                .unwrap_err();
            assert_eq!(error.code, tonic::Code::NotFound);
            assert_eq!(error.message, "game not found");
        }
    }

    #[tokio::test]
    async fn join_game_requires_the_running_status_and_the_right_password() {
        let sessions = MockSessions::new();
        let backend = backend(Arc::clone(&sessions), None);
        let created = backend
            .create_game(
                Uuid::new_v4(),
                CreateGameParams {
                    password: Some("sesame".to_string()),
                    ..params(2)
                },
            )
            .await
            .unwrap();
        let game_id = created.game.id.to_string();

        // Starting (not yet Running): FAILED_PRECONDITION with the legacy text.
        let error = backend
            .join_game(Uuid::new_v4(), &game_id, Some("sesame".to_string()))
            .await
            .unwrap_err();
        assert_eq!(error.code, tonic::Code::FailedPrecondition);
        assert_eq!(error.message, "game is not running (status: starting)");

        sessions
            .set_status(created.game.id, SessionStatus::Running);

        // Wrong password.
        let error = backend
            .join_game(Uuid::new_v4(), &game_id, Some("wrong".to_string()))
            .await
            .unwrap_err();
        assert_eq!(error.code, tonic::Code::PermissionDenied);
        assert_eq!(error.message, "invalid password");

        // Correct password: legacy ticket (password) without a session secret.
        let joined = backend
            .join_game(Uuid::new_v4(), &game_id, Some("sesame".to_string()))
            .await
            .unwrap();
        assert_eq!(joined.game_id, created.game.id);
        assert_eq!(joined.host, "127.0.0.1");
        assert_eq!(joined.port, created.game.port);
        assert_eq!(joined.ticket.kind, TicketKind::Legacy);
        assert_eq!(joined.ticket.value, "sesame");

        // The joiner is now a lobby player.
        assert_eq!(backend.list_games().await.unwrap()[0].players, 2);
    }

    #[tokio::test]
    async fn join_game_advertises_the_configured_host() {
        // `[server.grpc].announce_host` reaches `JoinGameResponse.host`; it is
        // independent of the listener's bind address.
        let sessions = MockSessions::new();
        let backend = backend(Arc::clone(&sessions), None).with_host("max.example.com");
        let created = backend
            .create_game(Uuid::new_v4(), params(2))
            .await
            .unwrap();
        sessions
            .set_status(created.game.id, SessionStatus::Running);

        let joined = backend
            .join_game(Uuid::new_v4(), &created.game.id.to_string(), None)
            .await
            .unwrap();

        assert_eq!(joined.host, "max.example.com");
        assert_eq!(joined.port, created.game.port);
    }

    #[tokio::test]
    async fn join_game_mints_a_signed_ticket_when_a_session_secret_is_configured() {
        let sessions = MockSessions::new();
        let backend = backend(Arc::clone(&sessions), Some("lobby-test-secret"));
        let created = backend
            .create_game(Uuid::new_v4(), params(2))
            .await
            .unwrap();
        sessions
            .set_status(created.game.id, SessionStatus::Running);

        let player_id = Uuid::new_v4();
        let joined = backend
            .join_game(player_id, &created.game.id.to_string(), None)
            .await
            .unwrap();

        assert_eq!(joined.ticket.kind, TicketKind::Signed);
        assert_eq!(joined.ticket.session_id, Some(created.game.id));
        assert_eq!(joined.ticket.player_id, Some(player_id));
        assert!(joined.ticket.value.starts_with("v1."));

        let verified = Auth::new("lobby-test-secret")
            .verify(&joined.ticket.value, created.game.id)
            .expect("the minted ticket must verify");
        assert_eq!(verified.player_id, player_id);
        assert_eq!(joined.ticket.expires_at_unix, Some(verified.expires_at_unix));
    }

    #[tokio::test]
    async fn unconfigured_backends_report_the_ws_messages() {
        let backend = MaxBackend::new(None, None, None, 32000..=32010);

        let error = backend.list_games().await.unwrap_err();
        assert_eq!(error.code, tonic::Code::Unavailable);
        assert_eq!(error.message, "max_lobby is not configured");

        let error = backend
            .admin_list_sessions(None)
            .await
            .unwrap_err();
        // Authorization runs first and is fail-closed.
        assert_eq!(error.code, tonic::Code::Unauthenticated);
        assert_eq!(
            error.message,
            "admin authorization is disabled: no admin_secret is configured"
        );
    }

    #[tokio::test]
    async fn admin_authorization_is_fail_closed() {
        let backend = backend(MockSessions::new(), None);
        let token = crate::admin::admin_token_for("admin-test-secret");

        let error = backend.admin_list_sessions(None).await.unwrap_err();
        assert_eq!(error.code, tonic::Code::Unauthenticated);
        assert_eq!(error.message, "missing admin_token");

        let error = backend
            .admin_list_sessions(Some("not-the-token".to_string()))
            .await
            .unwrap_err();
        assert_eq!(error.code, tonic::Code::Unauthenticated);
        assert_eq!(error.message, "invalid admin_token");

        assert!(backend.admin_list_sessions(Some(token)).await.is_ok());
    }

    #[tokio::test]
    async fn admin_status_maps_unknown_ids_to_unknown_and_validates_uuids() {
        let sessions = MockSessions::new();
        let backend = backend(Arc::clone(&sessions), None);
        let token = crate::admin::admin_token_for("admin-test-secret");

        let state = backend
            .admin_session_status(Some(token.clone()), &Uuid::new_v4().to_string())
            .await
            .unwrap();
        assert_eq!(state, GameStatus::Unknown);

        let error = backend
            .admin_session_status(Some(token), "abc")
            .await
            .unwrap_err();
        assert_eq!(error.code, tonic::Code::InvalidArgument);
        assert_eq!(error.message, "invalid game_id \"abc\": expected a UUID");
    }

    #[tokio::test]
    async fn admin_stop_and_restart_report_inline_errors_for_unknown_sessions() {
        let backend = backend(MockSessions::new(), None);
        let token = crate::admin::admin_token_for("admin-test-secret");
        let missing = Uuid::new_v4().to_string();

        let stopped = backend
            .admin_session_stop(Some(token.clone()), &missing)
            .await
            .unwrap();
        assert!(!stopped.ok);
        let error = stopped.error.expect("inline error");
        assert_eq!(error.code, tonic::Code::NotFound);
        assert_eq!(error.message, format!("session {missing} does not exist"));

        let restarted = backend
            .admin_session_restart(Some(token), &missing)
            .await
            .unwrap();
        assert!(!restarted.ok);
        assert!(restarted.new_game_id.is_none());
        assert_eq!(
            restarted.error.expect("inline error").code,
            tonic::Code::NotFound
        );
    }

    #[tokio::test]
    async fn admin_start_checks_the_port_range_and_collisions_before_starting() {
        let sessions = MockSessions::new();
        let sessions_handle = Arc::clone(&sessions);
        let backend = backend(Arc::clone(&sessions), None);
        let token = crate::admin::admin_token_for("admin-test-secret");

        // Outside MAX_PORT_RANGE: INVALID_ARGUMENT status (WS admin_error).
        let error = backend
            .admin_session_start(
                Some(token.clone()),
                AdminStartParams {
                    port: Some(4000),
                    game: GameOptions::default(),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, tonic::Code::InvalidArgument);
        assert!(error.message.contains("outside the configured MAX_PORT_RANGE"));

        // Running session on the first mock port: inline ok:false ALREADY_EXISTS.
        let first = sessions_handle
            .start(SessionConfig::default())
            .await
            .unwrap();
        let outcome = backend
            .admin_session_start(
                Some(token),
                AdminStartParams {
                    port: Some(u32::from(first.port)),
                    game: GameOptions::default(),
                },
            )
            .await
            .unwrap();
        assert!(!outcome.ok);
        assert_eq!(
            outcome.error.expect("inline error").code,
            tonic::Code::AlreadyExists
        );

        // A free port in range starts a session.
        let started = backend
            .admin_session_start(
                Some(crate::admin::admin_token_for("admin-test-secret")),
                AdminStartParams {
                    port: Some(32005),
                    game: GameOptions::default(),
                },
            )
            .await
            .unwrap();
        assert!(started.ok);
        assert!(started.game_id.is_some());
        assert_eq!(started.port, Some(32000 + 1));
    }

    #[tokio::test]
    async fn admin_start_reports_manager_failures_inline() {
        let sessions = MockSessions::new();
        sessions.set_start_error(Some(
            "no free UDP port left in the configured range".to_string(),
        ));
        let backend = backend(Arc::clone(&sessions), None);
        let token = crate::admin::admin_token_for("admin-test-secret");

        let outcome = backend
            .admin_session_start(
                Some(token),
                AdminStartParams {
                    port: None,
                    game: GameOptions::default(),
                },
            )
            .await
            .unwrap();

        assert!(!outcome.ok);
        let error = outcome.error.expect("inline error");
        assert_eq!(error.code, tonic::Code::ResourceExhausted);
        assert_eq!(error.message, "no free UDP port left in the configured range");
    }

    #[tokio::test]
    async fn admin_list_and_status_see_the_sessions_the_manager_tracks() {
        let sessions = MockSessions::new();
        let backend = backend(Arc::clone(&sessions), None);
        let token = crate::admin::admin_token_for("admin-test-secret");

        let created = backend
            .create_game(Uuid::new_v4(), params(2))
            .await
            .unwrap();

        let listed = backend
            .admin_list_sessions(Some(token.clone()))
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].game_id, created.game.id);
        assert_eq!(listed[0].state, GameStatus::Starting);
        assert!(listed[0].dir.contains(&created.game.id.to_string()));

        sessions
            .set_status(created.game.id, SessionStatus::Running);
        let state = backend
            .admin_session_status(Some(token), &created.game.id.to_string())
            .await
            .unwrap();
        assert_eq!(state, GameStatus::Running);
    }

    #[tokio::test]
    async fn from_config_file_creates_the_manager_from_the_max_section() {
        // The runtime path: `[max]` is read from the same config.toml the server
        // loaded, exactly like max_lobby/max_admin do in `on_init`.
        let temp = tempfile::tempdir().unwrap();
        let template = temp.path().join("template");
        std::fs::create_dir_all(&template).unwrap();
        std::fs::write(template.join("settings.json"), "{\"settings\": {}}").unwrap();
        let executable = temp.path().join("max-port.exe");
        std::fs::write(&executable, b"stub").unwrap();

        let config_path = temp.path().join("config.toml");
        std::fs::write(
            &config_path,
            format!(
                "[max]\nexecutable = {:?}\ntemplate_dir = {:?}\nsession_root = {:?}\n\
                 port_range_start = 32100\nport_range_end = 32110\n\
                 admin_secret = \"admin-secret\"\n",
                executable.to_string_lossy(),
                template.to_string_lossy(),
                temp.path().join("sessions").to_string_lossy(),
            ),
        )
        .unwrap();

        let backend = MaxBackend::from_config_file(&config_path);

        // A manager is configured (the lobby answers instead of "not configured").
        assert!(backend.list_games().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn unconfigured_max_config_leaves_the_adapter_unavailable() {
        // A `[max]` section without executable/template_dir means "not configured",
        // exactly like the plugins' `load_max_config` returning None.
        let backend = MaxBackend::from_max_config(MaxConfig::default());
        let error = backend.list_games().await.unwrap_err();
        assert_eq!(error.code, tonic::Code::Unavailable);
        assert_eq!(error.message, "max_lobby is not configured");
    }

    #[tokio::test]
    async fn ticket_info_parses_a_signed_value_without_the_secret() {
        let game_id = Uuid::new_v4();
        let player_id = Uuid::new_v4();
        let value = Auth::new("a-secret-only-the-issuer-knows").issue(game_id, player_id, TICKET_TTL);

        let ticket = ticket_info(value.clone(), game_id, player_id);
        assert_eq!(ticket.value, value);
        assert_eq!(ticket.kind, TicketKind::Signed);
        assert_eq!(ticket.session_id, Some(game_id));
        assert_eq!(ticket.player_id, Some(player_id));
        assert!(ticket.expires_at_unix.is_some());

        // A value that is not a signed ticket stays legacy.
        let legacy = ticket_info("sesame".to_string(), game_id, player_id);
        assert_eq!(legacy.kind, TicketKind::Legacy);
        assert_eq!(legacy.session_id, None);
    }
}
