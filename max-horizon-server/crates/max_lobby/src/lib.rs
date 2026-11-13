//! # max_lobby
//!
//! Lobby plugin for [M.A.X. Port] dedicated servers on the Horizon framework.
//!
//! The plugin maps three client events onto the [`max_session`] process manager:
//!
//! * `max_lobby:list_games` → `game_list` — the games currently registered in the lobby.
//! * `max_lobby:create_game` → `game_created` — starts a headless session and hands the
//!   session ticket to the creator.
//! * `max_lobby:join_game` → `game_joined` — validates the game and hands host/port/ticket
//!   to the joining player.
//!
//! ## Configuration
//!
//! The session manager is created lazily in `on_init` from two layered sources: the `[max]`
//! section of the Horizon `config.toml` (see `max_session::config`) and `MAX_*` environment
//! variables. Environment variables always win over the file.
//!
//! | Variable                 | Default         | Meaning                             |
//! |--------------------------|-----------------|-------------------------------------|
//! | `MAX_SERVER_EXE`         | *(required)*    | Path to the headless `max-port.exe` |
//! | `MAX_TEMPLATE_DIR`       | *(required)*    | Prepared session template directory |
//! | `MAX_SESSION_ROOT`       | `sessions`      | Root directory for session folders  |
//! | `MAX_GAME_DATA`          | *(empty)*       | Game data folder (MAX.RES)          |
//! | `MAX_PORT_RANGE`         | `32000-32100`   | Inclusive UDP port range            |
//! | `MAX_RESTARTS`           | `3`             | Watchdog restart limit              |
//! | `MAX_RESTART_BACKOFF_MS` | `2000`          | Initial restart backoff             |
//! | `MAX_SESSION_SECRET`     | *(empty)*       | HMAC secret for signed join tickets |
//!
//! Without `MAX_SERVER_EXE` / `MAX_TEMPLATE_DIR` (from the environment *or* `[max]`) — or when
//! the manager rejects the configuration — the plugin stays loaded, but every lobby event
//! answers `{"event": "error", "error": "max_lobby is not configured"}`.
//!
//! When a `session_secret` is configured, `join_game` mints a signed, per-player ticket with
//! [`max_auth`] (`v1.<game_id>.<player_id>.<expiry>.<hmac>`, 24 h TTL) and returns it in the
//! `ticket` field of `game_joined`; without a secret the legacy behavior (the password for
//! protected games, otherwise `""`) is kept.
//!
//! [M.A.X. Port]: https://github.com/klei1984/max

use async_trait::async_trait;
use horizon_event_system::{
    create_simple_plugin, current_timestamp, register_handlers, ClientConnectionRef,
    ClientEventWrapper, EventSystem, LogLevel, PlayerId, PluginError, ServerContext, SimplePlugin,
};
use max_session::{
    load_max_toml_section, MaxConfig, SessionConfig, SessionInfo, SessionManager, SessionStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use uuid::Uuid;

// ============================================================================
// Client event payloads
// ============================================================================

/// Payload of the `max_lobby:create_game` client event.
///
/// Every field is optional; unset fields keep the `SessionConfig` defaults. When a
/// `password` is given it becomes the session ticket verbatim and is never stored by the
/// lobby (only `password_protected` and a hash of the ticket are kept).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGameRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub world: Option<i64>,
    #[serde(default)]
    pub timer: Option<i64>,
    #[serde(default)]
    pub endturn: Option<i64>,
    #[serde(default)]
    pub play_mode: Option<i64>,
    #[serde(default)]
    pub start_gold: Option<i64>,
    #[serde(default)]
    pub victory_type: Option<i64>,
    #[serde(default)]
    pub victory_limit: Option<i64>,
    #[serde(default)]
    pub min_players: Option<u32>,
    #[serde(default)]
    pub password: Option<String>,
}

/// Payload of the `max_lobby:join_game` client event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinGameRequest {
    pub game_id: String,
    #[serde(default)]
    pub password: Option<String>,
}

// ============================================================================
// Lobby registry
// ============================================================================

/// A player registered in a lobby game.
#[derive(Debug, Clone)]
pub struct LobbyPlayer {
    pub player_id: PlayerId,
    pub name: String,
    pub joined_at: u64,
}

/// One lobby game: the session snapshot plus lobby-only metadata.
///
/// Tickets and passwords are deliberately absent: for protected games the ticket *is* the
/// password, so the registry only keeps [`LobbyGame::ticket_hash`] — never the plaintext.
pub struct LobbyGame {
    pub info: SessionInfo,
    pub world: i64,
    pub min_players: u32,
    pub password_protected: bool,
    pub created_by: PlayerId,
    pub players: Vec<LobbyPlayer>,
    /// Non-cryptographic hash of the session ticket, `Some` only for protected games.
    ticket_hash: Option<u64>,
}

/// Shared lobby state, guarded by a tokio mutex.
struct LobbyState {
    manager: Option<Arc<SessionManager>>,
    games: HashMap<Uuid, LobbyGame>,
    /// HMAC secret for signed per-player join tickets; `None` keeps the legacy behavior.
    session_secret: Option<String>,
}

impl LobbyState {
    fn new() -> Self {
        Self {
            manager: None,
            games: HashMap::new(),
            session_secret: None,
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// The M.A.X. Port lobby plugin.
pub struct MaxLobbyPlugin {
    name: String,
    state: Arc<Mutex<LobbyState>>,
}

impl MaxLobbyPlugin {
    pub fn new() -> Self {
        Self {
            name: "max_lobby".to_string(),
            state: Arc::new(Mutex::new(LobbyState::new())),
        }
    }
}

impl Default for MaxLobbyPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SimplePlugin for MaxLobbyPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        "0.1.0"
    }

    async fn register_handlers(
        &mut self,
        events: Arc<EventSystem>,
        context: Arc<dyn ServerContext>,
    ) -> Result<(), PluginError> {
        let state = Arc::clone(&self.state);
        let runtime = context.luminal_handle();

        // `list_games` takes no meaningful payload; `{}` is delivered as a JSON value.
        let list_handler = {
            let state = Arc::clone(&state);
            let runtime = runtime.clone();
            move |_event: ClientEventWrapper<serde_json::Value>,
                  _player_id: PlayerId,
                  connection: ClientConnectionRef| {
                let state = Arc::clone(&state);
                let _ = runtime.spawn(async move {
                    handle_list_games(state, connection).await;
                });
                Ok(())
            }
        };

        let create_handler = {
            let state = Arc::clone(&state);
            let runtime = runtime.clone();
            move |event: ClientEventWrapper<CreateGameRequest>,
                  player_id: PlayerId,
                  connection: ClientConnectionRef| {
                let state = Arc::clone(&state);
                let _ = runtime.spawn(async move {
                    handle_create_game(state, player_id, connection, event.data).await;
                });
                Ok(())
            }
        };

        let join_handler = {
            let state = Arc::clone(&state);
            let runtime = runtime.clone();
            move |event: ClientEventWrapper<JoinGameRequest>,
                  player_id: PlayerId,
                  connection: ClientConnectionRef| {
                let state = Arc::clone(&state);
                let _ = runtime.spawn(async move {
                    handle_join_game(state, player_id, connection, event.data).await;
                });
                Ok(())
            }
        };

        register_handlers!(events; client {
            "max_lobby", "list_games" => list_handler,
            "max_lobby", "create_game" => create_handler,
            "max_lobby", "join_game" => join_handler,
        })?;

        info!("max_lobby: registered handlers for max_lobby:list_games, max_lobby:create_game, max_lobby:join_game");
        Ok(())
    }

    async fn on_init(&mut self, context: Arc<dyn ServerContext>) -> Result<(), PluginError> {
        let mut state = self.state.lock().await;

        match load_max_config() {
            Some(config) => {
                let session_secret = config.session_secret.clone();
                match SessionManager::new(config) {
                    Ok(manager) => {
                        state.session_secret = session_secret;
                        state.manager = Some(manager);
                        context.log(
                            LogLevel::Info,
                            "max_lobby: session manager created; lobby is ready",
                        );
                    }
                    Err(error) => {
                        warn!(%error, "max_lobby: session manager rejected the configuration; lobby events will answer 'max_lobby is not configured'");
                        context.log(
                            LogLevel::Warn,
                            "max_lobby: session manager rejected the configuration; lobby events will answer 'max_lobby is not configured'",
                        );
                    }
                }
            }
            None => {
                warn!("max_lobby: executable / template_dir are not set (MAX_SERVER_EXE / MAX_TEMPLATE_DIR or the [max] config section); lobby events will answer 'max_lobby is not configured'");
                context.log(
                    LogLevel::Warn,
                    "max_lobby: executable / template_dir are not set (MAX_SERVER_EXE / MAX_TEMPLATE_DIR or the [max] config section); lobby events will answer 'max_lobby is not configured'",
                );
            }
        }

        Ok(())
    }

    async fn on_shutdown(&mut self, context: Arc<dyn ServerContext>) -> Result<(), PluginError> {
        let mut state = self.state.lock().await;

        if let Some(manager) = state.manager.take() {
            manager.shutdown_all().await;
            context.log(LogLevel::Info, "max_lobby: all game sessions stopped");
        }

        state.games.clear();
        context.log(LogLevel::Info, "max_lobby: shutdown complete");
        Ok(())
    }
}

// ============================================================================
// Event handlers (run on the luminal runtime of the host)
// ============================================================================

async fn handle_list_games(state: Arc<Mutex<LobbyState>>, connection: ClientConnectionRef) {
    let mut lobby = state.lock().await;
    let Some(manager) = lobby.manager.clone() else {
        lobby_error(&connection, "max_lobby is not configured").await;
        return;
    };

    // Drop registry entries whose session the manager no longer tracks (stopped, or the
    // watchdog gave up on restarts); otherwise the lobby would list dead games forever.
    let gone: Vec<Uuid> = lobby
        .games
        .iter()
        .filter(|(id, _)| manager.health(**id).is_none())
        .map(|(id, _)| *id)
        .collect();
    for id in &gone {
        lobby.games.remove(id);
    }

    let mut games = Vec::with_capacity(lobby.games.len());
    for (id, game) in lobby.games.iter() {
        let status = manager.health(*id).unwrap_or(SessionStatus::Stopped);
        games.push(game_json(*id, game, &status));
    }

    let payload = serde_json::json!({ "event": "game_list", "games": games });
    if let Err(error) = connection.respond_json(&payload).await {
        warn!(%error, "max_lobby: failed to send game_list response");
    }
}

async fn handle_create_game(
    state: Arc<Mutex<LobbyState>>,
    player_id: PlayerId,
    connection: ClientConnectionRef,
    request: CreateGameRequest,
) {
    let mut lobby = state.lock().await;
    let Some(manager) = lobby.manager.clone() else {
        lobby_error(&connection, "max_lobby is not configured").await;
        return;
    };

    let mut config = SessionConfig::default();
    if let Some(name) = request.name {
        config.name = Some(name);
    }
    if let Some(value) = request.world {
        config.world = value;
    }
    if let Some(value) = request.timer {
        config.timer = value;
    }
    if let Some(value) = request.endturn {
        config.endturn = value;
    }
    if let Some(value) = request.play_mode {
        config.play_mode = value;
    }
    if let Some(value) = request.start_gold {
        config.start_gold = value;
    }
    if let Some(value) = request.victory_type {
        config.victory_type = value;
    }
    if let Some(value) = request.victory_limit {
        config.victory_limit = value;
    }
    if let Some(value) = request.min_players {
        config.min_players = value;
    }

    // A network game needs at least two human players; the headless server waits for
    // `server_min_players` clients before it starts, and a single-player session is just a
    // local game with an extra relay.
    if config.min_players < 2 {
        lobby_error(&connection, "a network game needs at least 2 players").await;
        return;
    }

    // The password doubles as the session ticket for protected games.
    let password_protected = request.password.is_some();
    config.password_protected = password_protected;
    if let Some(password) = &request.password {
        config.ticket = Some(password.clone());
    }

    let (world, min_players) = (config.world, config.min_players);

    match manager.start(config).await {
        Err(error) => {
            warn!(%error, "max_lobby: failed to start a game session");
            lobby_error(&connection, &error.to_string()).await;
        }
        Ok(info) => {
            let ticket = request.password.clone().unwrap_or_default();
            lobby.games.insert(
                info.id,
                LobbyGame {
                    info: info.clone(),
                    world,
                    min_players,
                    password_protected,
                    created_by: player_id,
                    // The creator is a player of their own game.
                    players: vec![LobbyPlayer {
                        player_id,
                        name: player_id.to_string(),
                        joined_at: current_timestamp(),
                    }],
                    ticket_hash: password_protected.then(|| ticket_hash(&ticket)),
                },
            );

            let status = manager.health(info.id).unwrap_or(SessionStatus::Starting);
            let game = lobby.games.get(&info.id).expect("game just inserted");
            let payload = serde_json::json!({
                "event": "game_created",
                "game": game_json(info.id, game, &status),
                "ticket": ticket,
            });
            if let Err(error) = connection.respond_json(&payload).await {
                warn!(%error, "max_lobby: failed to send game_created response");
            }
        }
    }
}

async fn handle_join_game(
    state: Arc<Mutex<LobbyState>>,
    player_id: PlayerId,
    connection: ClientConnectionRef,
    request: JoinGameRequest,
) {
    let mut lobby = state.lock().await;
    let Some(manager) = lobby.manager.clone() else {
        lobby_error(&connection, "max_lobby is not configured").await;
        return;
    };

    let game_id = match Uuid::parse_str(&request.game_id) {
        Ok(id) => id,
        Err(_) => {
            lobby_error(&connection, "game not found").await;
            return;
        }
    };

    let Some(game) = lobby.games.get(&game_id) else {
        lobby_error(&connection, "game not found").await;
        return;
    };

    // Protected games are joined by comparing hashes: the ticket equals the password, and
    // the plaintext is never stored in the registry.
    if game.password_protected {
        let matches = match (&request.password, game.ticket_hash) {
            (Some(password), Some(expected)) => ticket_hash(password) == expected,
            _ => false,
        };
        if !matches {
            lobby_error(&connection, "invalid password").await;
            return;
        }
    }

    match manager.health(game_id) {
        Some(SessionStatus::Running) => {}
        Some(status) => {
            lobby_error(
                &connection,
                &format!("game is not running (status: {})", status_str(&status)),
            )
            .await;
            return;
        }
        None => {
            lobby_error(&connection, "game is not running").await;
            return;
        }
    }

    // With a configured session secret the join answer carries a signed, per-player ticket
    // (`v1.<game_id>.<player_id>.<expiry>.<hmac>`, see max_auth). Without one the legacy
    // behavior stays: the password is the ticket for protected games, `""` for open ones.
    // `game_id`, `host` and `port` stay plain transport metadata that the ticket covers
    // separately (the ticket binds to `game_id`).
    let port = game.info.port;
    let legacy_ticket = if game.password_protected {
        request.password.clone().unwrap_or_default()
    } else {
        String::new()
    };
    let ticket = join_ticket(lobby.session_secret.as_deref(), game_id, player_id, legacy_ticket);

    if let Some(game) = lobby.games.get_mut(&game_id) {
        if !game.players.iter().any(|p| p.player_id == player_id) {
            game.players.push(LobbyPlayer {
                player_id,
                name: player_id.to_string(),
                joined_at: current_timestamp(),
            });
        }
    }

    debug!(game_id = %game_id, created_by = %lobby.games[&game_id].created_by, "max_lobby: player joined a game");

    let payload = serde_json::json!({
        "event": "game_joined",
        "game_id": game_id.to_string(),
        "host": "127.0.0.1",
        "port": port,
        "ticket": ticket,
    });
    if let Err(error) = connection.respond_json(&payload).await {
        warn!(%error, "max_lobby: failed to send game_joined response");
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Sends the standard error response to one client.
async fn lobby_error(connection: &ClientConnectionRef, message: &str) {
    let payload = serde_json::json!({ "event": "error", "error": message });
    if let Err(error) = connection.respond_json(&payload).await {
        warn!(%error, message, "max_lobby: failed to send error response");
    }
}

/// The lobby view of one game, as sent inside `game_list` / `game_created`.
fn game_json(id: Uuid, game: &LobbyGame, status: &SessionStatus) -> serde_json::Value {
    serde_json::json!({
        "id": id.to_string(),
        "name": game.info.name,
        "world": game.world,
        "status": status_str(status),
        "port": game.info.port,
        "players": game.players.len(),
        "min_players": game.min_players,
        "password_protected": game.password_protected,
    })
}

/// Wire names of the session lifecycle states.
fn status_str(status: &SessionStatus) -> &'static str {
    match status {
        SessionStatus::Starting => "starting",
        SessionStatus::Running => "running",
        SessionStatus::Exited(_) => "exited",
        SessionStatus::Stopped => "stopped",
    }
}

/// The ticket handed to a player joining `game_id`.
///
/// With a (non-empty) session secret this is a signed, per-player ticket from `max_auth`
/// (`v1.<game_id>.<player_id>.<expiry>.<hmac>`, 24 h TTL). Without a secret the legacy
/// `fallback` is returned verbatim — the password for protected games, `""` for open ones.
fn join_ticket(
    session_secret: Option<&str>,
    game_id: Uuid,
    player_id: PlayerId,
    fallback: String,
) -> String {
    match session_secret.filter(|secret| !secret.is_empty()) {
        Some(secret) => {
            let auth = max_auth::Auth::new(secret);
            auth.issue(game_id, player_id.0, Duration::from_secs(24 * 60 * 60))
        }
        None => fallback,
    }
}

/// Non-cryptographic hash of a session ticket.
///
/// The registry never stores tickets (which double as passwords for protected games) in
/// plaintext; this hash lets `join_game` verify a supplied password and hand back the real
/// ticket — which is the password itself — without keeping the secret.
fn ticket_hash(ticket: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ticket.hash(&mut hasher);
    hasher.finish()
}

// ============================================================================
// Configuration loading
// ============================================================================

/// Loads the manager configuration from the `[max]` section of `config.toml` and the `MAX_*`
/// environment variables, or `None` when the required values (`executable`, `template_dir`)
/// are missing from both sources.
///
/// Environment variables have priority over the file, so existing environment-configured
/// deployments keep working unchanged. An unreadable `config.toml` or an invalid `[max]`
/// section is logged and falls back to the environment alone.
fn load_max_config() -> Option<MaxConfig> {
    let section = match load_max_toml_section(Path::new("config.toml")) {
        Ok(section) => Some(section),
        Err(error) => {
            warn!(%error, "max_lobby: ignoring config.toml [max] section; using MAX_* environment variables only");
            None
        }
    };

    let config = match MaxConfig::from_env_and_toml(section.as_ref()) {
        Ok(config) => config,
        Err(error) => {
            warn!(%error, "max_lobby: invalid [max] configuration; using MAX_* environment variables only");
            MaxConfig::from_env_and_toml(None).expect("environment-only load cannot fail")
        }
    };

    if config.executable.as_os_str().is_empty() || config.template_dir.as_os_str().is_empty() {
        return None;
    }

    Some(config)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_range_parses() {
        // The parser lives in max_session::config now (shared with the [max] loader); the
        // lobby uses it through `MaxConfig::apply_env`.
        let parse = max_session::config::parse_port_range;
        assert_eq!(parse("32000-32100"), Some(32000..=32100));
        assert_eq!(parse(" 10 - 20 "), Some(10..=20));
        assert_eq!(parse("20-10"), None);
        assert_eq!(parse("10"), None);
        assert_eq!(parse("abc"), None);
    }

    #[test]
    fn join_ticket_is_a_verifiable_signed_ticket_when_a_secret_is_set() {
        let secret = "lobby-unit-test-secret";
        let game_id = Uuid::new_v4();
        let player_id = PlayerId::new();

        let ticket = join_ticket(Some(secret), game_id, player_id, String::from("fallback"));

        let parsed = max_auth::Auth::new(secret)
            .verify(&ticket, game_id)
            .expect("a fresh ticket must verify");
        assert_eq!(parsed.session_id, game_id);
        assert_eq!(parsed.player_id, player_id.0);
        assert_ne!(ticket, "fallback");

        // The same ticket must be rejected for a different game.
        assert!(max_auth::Auth::new(secret)
            .verify(&ticket, Uuid::new_v4())
            .is_err());
    }

    #[test]
    fn join_ticket_falls_back_to_the_legacy_value_without_a_secret() {
        let game_id = Uuid::new_v4();
        let player_id = PlayerId::new();

        assert_eq!(
            join_ticket(None, game_id, player_id, String::from("legacy")),
            "legacy"
        );
        assert_eq!(
            join_ticket(Some(""), game_id, player_id, String::from("legacy")),
            "legacy"
        );
    }

    #[test]
    fn ticket_hash_is_stable_and_distinguishes_tickets() {
        assert_eq!(ticket_hash("secret"), ticket_hash("secret"));
        assert_ne!(ticket_hash("secret"), ticket_hash("other"));
    }

    #[test]
    fn status_names_match_the_wire_format() {
        assert_eq!(status_str(&SessionStatus::Starting), "starting");
        assert_eq!(status_str(&SessionStatus::Running), "running");
        assert_eq!(status_str(&SessionStatus::Exited(1)), "exited");
        assert_eq!(status_str(&SessionStatus::Stopped), "stopped");
    }
}

// Create the plugin using our macro - zero unsafe code!
create_simple_plugin!(MaxLobbyPlugin);
