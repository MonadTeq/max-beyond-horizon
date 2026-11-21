//! # max_admin
//!
//! Admin plugin for [M.A.X. Port] dedicated servers on the Horizon framework.
//!
//! The Horizon admin panel gives a UI, but automation and CLI tooling need *programmatic*
//! events instead. This plugin maps five client events onto the [`max_session`] process
//! manager:
//!
//! | Client event                  | Response          | Payload                                        |
//! |-------------------------------|-------------------|------------------------------------------------|
//! | `max_admin:list_sessions`     | `sessions_listed` | `{"sessions":[{"game_id","name","port","state","dir"}]}` |
//! | `max_admin:session_status`    | `session_status`  | `{"game_id","status":{"state"}}`               |
//! | `max_admin:stop_session`      | `session_stopped` | `{"game_id","ok"}`                             |
//! | `max_admin:restart_session`   | `session_restarted` | `{"game_id","ok"}` (+`new_game_id`,`port`)   |
//! | `max_admin:start_session`     | `session_started` | `{"game_id","port"}` (or `ok:false`+`error`)  |
//!
//! Every handler answers JSON through [`ClientConnectionRef::respond_json`]; invalid input
//! (missing, mistyped or unknown fields) answers `{"event":"admin_error","error":"..."}`
//! instead of panicking or failing silently.
//!
//! ## Authorization
//!
//! Every admin event additionally accepts an optional `admin_token` field. A token is
//! `hex(HMAC-SHA256(admin_secret, "max-admin"))`, compared in constant time. Authorization is
//! fail-closed: without a configured `admin_secret` (or with a missing/wrong token) the event
//! answers `{"event":"admin_error","error":"..."}` and no session state is touched.
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
//! | `MAX_ADMIN_SECRET`       | *(empty)*       | HMAC secret for admin tokens        |
//!
//! Without `MAX_SERVER_EXE` / `MAX_TEMPLATE_DIR` (from the environment *or* `[max]`) — or when
//! the manager rejects the configuration — the plugin stays loaded, but every admin event
//! answers `{"event": "admin_error", "error": "max_admin is not configured"}`.
//!
//! [M.A.X. Port]: https://github.com/klei1984/max

use async_trait::async_trait;
use hmac::{Hmac, Mac};
use horizon_event_system::{
    create_simple_plugin, register_handlers, ClientConnectionRef, ClientEventWrapper,
    EventSystem, LogLevel, PlayerId, PluginError, ServerContext, SimplePlugin,
};
use max_session::{
    load_max_toml_section, MaxConfig, SessionConfig, SessionInfo, SessionManager, SessionStatus,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

// ============================================================================
// Client event payloads
// ============================================================================

/// Payload of the `max_admin:session_status` / `max_admin:stop_session` /
/// `max_admin:restart_session` client events: the id of the session to act on, plus the
/// optional `admin_token` (see [`authorize_admin`]).
///
/// Unknown fields are rejected so typos surface as `admin_error` instead of being
/// silently ignored.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameIdRequest {
    pub game_id: String,
    /// Admin authorization token: `hex(HMAC-SHA256(admin_secret, "max-admin"))`.
    #[serde(default)]
    pub admin_token: Option<String>,
}

/// Payload of the `max_admin:start_session` client event.
///
/// Every field is optional; unset fields keep the `SessionConfig` defaults. The optional
/// `port` is a *conflict pre-check*: `SessionManager` always allocates ports itself from
/// `MAX_PORT_RANGE`, so a requested port is validated against the range and against the
/// ports of running sessions, but the session still receives an allocated port (returned in
/// the response).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartSessionRequest {
    /// Admin authorization token: `hex(HMAC-SHA256(admin_secret, "max-admin"))`.
    #[serde(default)]
    pub admin_token: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
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
}

// ============================================================================
// Admin state
// ============================================================================

/// Shared admin state, guarded by a tokio mutex.
struct AdminState {
    manager: Option<Arc<SessionManager>>,
    port_range: std::ops::RangeInclusive<u16>,
    /// HMAC secret for admin tokens; `None` fails every admin event closed.
    admin_secret: Option<String>,
}

impl AdminState {
    fn new() -> Self {
        Self {
            manager: None,
            port_range: 32000..=32100,
            admin_secret: None,
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// The M.A.X. Port admin plugin.
pub struct MaxAdminPlugin {
    name: String,
    state: Arc<Mutex<AdminState>>,
}

impl MaxAdminPlugin {
    pub fn new() -> Self {
        Self {
            name: "max_admin".to_string(),
            state: Arc::new(Mutex::new(AdminState::new())),
        }
    }
}

impl Default for MaxAdminPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SimplePlugin for MaxAdminPlugin {
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

        // All five handlers take `ClientEventWrapper<serde_json::Value>` and validate the
        // payload by hand: a typed wrapper would make the event system silently skip the
        // handler on malformed input, while we promise an `admin_error` answer instead.
        // The `admin_token` field is extracted before typed validation so authorization
        // fails closed even for malformed payloads.
        let list_handler = {
            let state = Arc::clone(&state);
            let runtime = runtime.clone();
            move |event: ClientEventWrapper<serde_json::Value>,
                  _player_id: PlayerId,
                  connection: ClientConnectionRef| {
                let state = Arc::clone(&state);
                let _ = runtime.spawn(async move {
                    handle_list_sessions(state, connection, event.data).await;
                });
                Ok(())
            }
        };

        let status_handler = {
            let state = Arc::clone(&state);
            let runtime = runtime.clone();
            move |event: ClientEventWrapper<serde_json::Value>,
                  _player_id: PlayerId,
                  connection: ClientConnectionRef| {
                let state = Arc::clone(&state);
                let _ = runtime.spawn(async move {
                    handle_session_status(state, connection, event.data).await;
                });
                Ok(())
            }
        };

        let stop_handler = {
            let state = Arc::clone(&state);
            let runtime = runtime.clone();
            move |event: ClientEventWrapper<serde_json::Value>,
                  _player_id: PlayerId,
                  connection: ClientConnectionRef| {
                let state = Arc::clone(&state);
                let _ = runtime.spawn(async move {
                    handle_stop_session(state, connection, event.data).await;
                });
                Ok(())
            }
        };

        let restart_handler = {
            let state = Arc::clone(&state);
            let runtime = runtime.clone();
            move |event: ClientEventWrapper<serde_json::Value>,
                  _player_id: PlayerId,
                  connection: ClientConnectionRef| {
                let state = Arc::clone(&state);
                let _ = runtime.spawn(async move {
                    handle_restart_session(state, connection, event.data).await;
                });
                Ok(())
            }
        };

        let start_handler = {
            let state = Arc::clone(&state);
            let runtime = runtime.clone();
            move |event: ClientEventWrapper<serde_json::Value>,
                  _player_id: PlayerId,
                  connection: ClientConnectionRef| {
                let state = Arc::clone(&state);
                let _ = runtime.spawn(async move {
                    handle_start_session(state, connection, event.data).await;
                });
                Ok(())
            }
        };

        register_handlers!(events; client {
            "max_admin", "list_sessions" => list_handler,
            "max_admin", "session_status" => status_handler,
            "max_admin", "stop_session" => stop_handler,
            "max_admin", "restart_session" => restart_handler,
            "max_admin", "start_session" => start_handler,
        })?;

        info!("max_admin: registered handlers for max_admin:list_sessions, max_admin:session_status, max_admin:stop_session, max_admin:restart_session, max_admin:start_session");
        Ok(())
    }

    async fn on_init(&mut self, context: Arc<dyn ServerContext>) -> Result<(), PluginError> {
        let mut state = self.state.lock().await;

        match load_max_config() {
            Some(config) => {
                state.port_range = config.port_range.clone();
                let admin_secret = config.admin_secret.clone();
                match SessionManager::new(config) {
                    Ok(manager) => {
                        state.admin_secret = admin_secret;
                        state.manager = Some(manager);
                        context.log(
                            LogLevel::Info,
                            "max_admin: session manager created; admin events are ready",
                        );
                    }
                    Err(error) => {
                        warn!(%error, "max_admin: session manager rejected the configuration; admin events will answer 'max_admin is not configured'");
                        context.log(
                            LogLevel::Warn,
                            "max_admin: session manager rejected the configuration; admin events will answer 'max_admin is not configured'",
                        );
                    }
                }
            }
            None => {
                warn!("max_admin: executable / template_dir are not set (MAX_SERVER_EXE / MAX_TEMPLATE_DIR or the [max] config section); admin events will answer 'max_admin is not configured'");
                context.log(
                    LogLevel::Warn,
                    "max_admin: executable / template_dir are not set (MAX_SERVER_EXE / MAX_TEMPLATE_DIR or the [max] config section); admin events will answer 'max_admin is not configured'",
                );
            }
        }

        Ok(())
    }

    async fn on_shutdown(&mut self, context: Arc<dyn ServerContext>) -> Result<(), PluginError> {
        let mut state = self.state.lock().await;

        if let Some(manager) = state.manager.take() {
            manager.shutdown_all().await;
            context.log(LogLevel::Info, "max_admin: all game sessions stopped");
        }

        context.log(LogLevel::Info, "max_admin: shutdown complete");
        Ok(())
    }
}

// ============================================================================
// Event handlers (run on the luminal runtime of the host)
// ============================================================================

async fn handle_list_sessions(
    state: Arc<Mutex<AdminState>>,
    connection: ClientConnectionRef,
    payload: serde_json::Value,
) {
    {
        let admin = state.lock().await;
        if let Err(error) = authorize_admin(
            admin.admin_secret.as_deref(),
            extract_admin_token(&payload).as_deref(),
        ) {
            admin_error(&connection, error).await;
            return;
        }
    }

    let admin = state.lock().await;
    let Some(manager) = admin.manager.clone() else {
        admin_error(&connection, "max_admin is not configured").await;
        return;
    };

    let sessions: Vec<serde_json::Value> = manager.list().iter().map(session_json).collect();
    let payload = serde_json::json!({ "event": "sessions_listed", "sessions": sessions });
    if let Err(error) = connection.respond_json(&payload).await {
        warn!(%error, "max_admin: failed to send sessions_listed response");
    }
}

async fn handle_session_status(
    state: Arc<Mutex<AdminState>>,
    connection: ClientConnectionRef,
    payload: serde_json::Value,
) {
    {
        let admin = state.lock().await;
        if let Err(error) = authorize_admin(
            admin.admin_secret.as_deref(),
            extract_admin_token(&payload).as_deref(),
        ) {
            admin_error(&connection, error).await;
            return;
        }
    }

    let request: GameIdRequest = match parse_request("session_status", payload) {
        Ok(request) => request,
        Err(error) => {
            admin_error(&connection, &error).await;
            return;
        }
    };

    let game_id = match Uuid::parse_str(&request.game_id) {
        Ok(id) => id,
        Err(_) => {
            admin_error(
                &connection,
                &format!("invalid game_id {:?}: expected a UUID", request.game_id),
            )
            .await;
            return;
        }
    };

    let admin = state.lock().await;
    let Some(manager) = admin.manager.clone() else {
        admin_error(&connection, "max_admin is not configured").await;
        return;
    };

    let state = manager
        .health(game_id)
        .map(|status| status_str(&status))
        .unwrap_or("unknown");

    let payload = serde_json::json!({
        "event": "session_status",
        "game_id": request.game_id,
        "status": { "state": state },
    });
    if let Err(error) = connection.respond_json(&payload).await {
        warn!(%error, "max_admin: failed to send session_status response");
    }
}

async fn handle_stop_session(
    state: Arc<Mutex<AdminState>>,
    connection: ClientConnectionRef,
    payload: serde_json::Value,
) {
    {
        let admin = state.lock().await;
        if let Err(error) = authorize_admin(
            admin.admin_secret.as_deref(),
            extract_admin_token(&payload).as_deref(),
        ) {
            admin_error(&connection, error).await;
            return;
        }
    }

    let request: GameIdRequest = match parse_request("stop_session", payload) {
        Ok(request) => request,
        Err(error) => {
            admin_error(&connection, &error).await;
            return;
        }
    };

    let game_id = match Uuid::parse_str(&request.game_id) {
        Ok(id) => id,
        Err(_) => {
            admin_error(
                &connection,
                &format!("invalid game_id {:?}: expected a UUID", request.game_id),
            )
            .await;
            return;
        }
    };

    let admin = state.lock().await;
    let Some(manager) = admin.manager.clone() else {
        admin_error(&connection, "max_admin is not configured").await;
        return;
    };

    let payload = match manager.stop(game_id).await {
        Ok(()) => {
            info!(session = %game_id, "max_admin: session stopped");
            serde_json::json!({
                "event": "session_stopped",
                "game_id": request.game_id,
                "ok": true,
            })
        }
        Err(error) => {
            warn!(session = %game_id, %error, "max_admin: failed to stop session");
            serde_json::json!({
                "event": "session_stopped",
                "game_id": request.game_id,
                "ok": false,
                "error": error.to_string(),
            })
        }
    };
    if let Err(error) = connection.respond_json(&payload).await {
        warn!(%error, "max_admin: failed to send session_stopped response");
    }
}

async fn handle_restart_session(
    state: Arc<Mutex<AdminState>>,
    connection: ClientConnectionRef,
    payload: serde_json::Value,
) {
    {
        let admin = state.lock().await;
        if let Err(error) = authorize_admin(
            admin.admin_secret.as_deref(),
            extract_admin_token(&payload).as_deref(),
        ) {
            admin_error(&connection, error).await;
            return;
        }
    }

    let request: GameIdRequest = match parse_request("restart_session", payload) {
        Ok(request) => request,
        Err(error) => {
            admin_error(&connection, &error).await;
            return;
        }
    };

    let game_id = match Uuid::parse_str(&request.game_id) {
        Ok(id) => id,
        Err(_) => {
            admin_error(
                &connection,
                &format!("invalid game_id {:?}: expected a UUID", request.game_id),
            )
            .await;
            return;
        }
    };

    let admin = state.lock().await;
    let Some(manager) = admin.manager.clone() else {
        admin_error(&connection, "max_admin is not configured").await;
        return;
    };

    // A restart stops the old session and starts a fresh one: the id and port change.
    let payload = match manager.restart(game_id).await {
        Ok(info) => {
            info!(session = %game_id, new_session = %info.id, port = info.port, "max_admin: session restarted");
            serde_json::json!({
                "event": "session_restarted",
                "game_id": request.game_id,
                "ok": true,
                "new_game_id": info.id.to_string(),
                "port": info.port,
            })
        }
        Err(error) => {
            warn!(session = %game_id, %error, "max_admin: failed to restart session");
            serde_json::json!({
                "event": "session_restarted",
                "game_id": request.game_id,
                "ok": false,
                "error": error.to_string(),
            })
        }
    };
    if let Err(error) = connection.respond_json(&payload).await {
        warn!(%error, "max_admin: failed to send session_restarted response");
    }
}

async fn handle_start_session(
    state: Arc<Mutex<AdminState>>,
    connection: ClientConnectionRef,
    payload: serde_json::Value,
) {
    {
        let admin = state.lock().await;
        if let Err(error) = authorize_admin(
            admin.admin_secret.as_deref(),
            extract_admin_token(&payload).as_deref(),
        ) {
            admin_error(&connection, error).await;
            return;
        }
    }

    let request: StartSessionRequest = match parse_request("start_session", payload) {
        Ok(request) => request,
        Err(error) => {
            admin_error(&connection, &error).await;
            return;
        }
    };

    let admin = state.lock().await;
    let Some(manager) = admin.manager.clone() else {
        admin_error(&connection, "max_admin is not configured").await;
        return;
    };

    // Optional port pre-check: `SessionManager` allocates ports itself, so a requested port
    // cannot be pinned — but we can still reject requests that are outside the configured
    // range or collide with a running session before spending a start attempt.
    if let Some(port) = request.port {
        if !admin.port_range.contains(&port) {
            let message = format!(
                "requested port {port} is outside the configured MAX_PORT_RANGE ({}..={})",
                admin.port_range.start(),
                admin.port_range.end()
            );
            admin_error(&connection, &message).await;
            return;
        }
        if let Some(existing) = manager.list().iter().find(|info| info.port == port) {
            let payload = serde_json::json!({
                "event": "session_started",
                "ok": false,
                "error": format!("port {port} is already used by session {}", existing.id),
            });
            if let Err(error) = connection.respond_json(&payload).await {
                warn!(%error, "max_admin: failed to send session_started response");
            }
            return;
        }
    }

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

    // A failure here is the real port-conflict path: the manager answers "no free UDP port
    // left in the configured range" once every port of `MAX_PORT_RANGE` is taken.
    let payload = match manager.start(config).await {
        Ok(info) => {
            info!(session = %info.id, port = info.port, "max_admin: session started");
            serde_json::json!({
                "event": "session_started",
                "game_id": info.id.to_string(),
                "port": info.port,
            })
        }
        Err(error) => {
            warn!(%error, "max_admin: failed to start a session");
            serde_json::json!({
                "event": "session_started",
                "ok": false,
                "error": error.to_string(),
            })
        }
    };
    if let Err(error) = connection.respond_json(&payload).await {
        warn!(%error, "max_admin: failed to send session_started response");
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// HMAC-SHA256 used for admin tokens.
type HmacSha256 = Hmac<Sha256>;

/// Block size of SHA-256 in bytes; HMAC keys longer than this are hashed down first, as
/// RFC 2104 prescribes (and because `HmacSha256::new_from_slice` rejects longer keys).
const SHA256_BLOCK_LEN: usize = 64;

/// Admin authorization.
///
/// The token scheme is `token = hex(HMAC-SHA256(admin_secret, "max-admin"))`, compared in
/// constant time. Authorization is fail-closed: without a configured `admin_secret` every
/// event is rejected, so an unconfigured deployment never exposes admin commands.
fn authorize_admin(
    admin_secret: Option<&str>,
    admin_token: Option<&str>,
) -> Result<(), &'static str> {
    let Some(secret) = admin_secret else {
        return Err("admin authorization is disabled: no admin_secret is configured");
    };
    let Some(token) = admin_token else {
        return Err("missing admin_token");
    };
    if verify_admin_token(secret, token) {
        Ok(())
    } else {
        Err("invalid admin_token")
    }
}

/// Pulls the optional `admin_token` field out of a raw client payload. A payload that is not
/// an object — or whose `admin_token` is not a string — yields `None`.
fn extract_admin_token(payload: &serde_json::Value) -> Option<String> {
    payload
        .get("admin_token")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// Mints the admin token for `secret`: lowercase hex of `HMAC-SHA256(secret, "max-admin")`.
///
/// This is the operator-facing half of the admin token scheme: tooling computes the token
/// with this function (or any HMAC-SHA256 implementation) and sends it in the `admin_token`
/// field of an admin event.
pub fn admin_token_for(secret: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(&effective_hmac_key(secret.as_bytes()))
        .expect("effective_hmac_key never exceeds the SHA-256 block size");
    mac.update(b"max-admin");
    encode_hex(&mac.finalize().into_bytes())
}

/// Constant-time check of a presented admin token.
fn verify_admin_token(secret: &str, token: &str) -> bool {
    let Some(expected) = decode_hex(token) else {
        return false;
    };
    let mut mac = HmacSha256::new_from_slice(&effective_hmac_key(secret.as_bytes()))
        .expect("effective_hmac_key never exceeds the SHA-256 block size");
    mac.update(b"max-admin");
    mac.verify_slice(&expected).is_ok()
}

/// The effective HMAC key: keys longer than the SHA-256 block size are hashed down
/// (RFC 2104 step 1), which guarantees `HmacSha256::new_from_slice` never fails.
fn effective_hmac_key(secret: &[u8]) -> Vec<u8> {
    if secret.len() > SHA256_BLOCK_LEN {
        let mut hasher = Sha256::new();
        hasher.update(secret);
        hasher.finalize().to_vec()
    } else {
        secret.to_vec()
    }
}

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Lowercase hex encoding.
fn encode_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX_DIGITS[(byte >> 4) as usize] as char);
        out.push(HEX_DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Checked hex decoding; accepts upper- and lowercase digits, rejects odd lengths.
fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    let bytes = hex.as_bytes();
    if bytes.is_empty() || bytes.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        out.push((hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?);
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Validates a raw client payload against `T` (missing fields, mistyped fields and —
/// thanks to `#[serde(deny_unknown_fields)]` — unknown fields all become errors).
fn parse_request<T: serde::de::DeserializeOwned>(
    event_name: &str,
    payload: serde_json::Value,
) -> Result<T, String> {
    serde_json::from_value::<T>(payload)
        .map_err(|error| format!("invalid max_admin:{event_name} payload: {error}"))
}

/// Sends the standard admin error response to one client.
async fn admin_error(connection: &ClientConnectionRef, message: &str) {
    let payload = serde_json::json!({ "event": "admin_error", "error": message });
    if let Err(error) = connection.respond_json(&payload).await {
        warn!(%error, message, "max_admin: failed to send admin_error response");
    }
}

/// The admin view of one session, as sent inside `sessions_listed`.
fn session_json(info: &SessionInfo) -> serde_json::Value {
    serde_json::json!({
        "game_id": info.id.to_string(),
        "name": info.name,
        "port": info.port,
        "state": status_str(&info.status),
        "dir": info.dir.to_string_lossy(),
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
            warn!(%error, "max_admin: ignoring config.toml [max] section; using MAX_* environment variables only");
            None
        }
    };

    let config = match MaxConfig::from_env_and_toml(section.as_ref()) {
        Ok(config) => config,
        Err(error) => {
            warn!(%error, "max_admin: invalid [max] configuration; using MAX_* environment variables only");
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
        // admin plugin uses it through `MaxConfig::apply_env`.
        let parse = max_session::config::parse_port_range;
        assert_eq!(parse("32000-32100"), Some(32000..=32100));
        assert_eq!(parse(" 10 - 20 "), Some(10..=20));
        assert_eq!(parse("20-10"), None);
        assert_eq!(parse("10"), None);
        assert_eq!(parse("abc"), None);
    }

    #[test]
    fn admin_authorization_accepts_only_the_correct_token() {
        let secret = "admin-unit-test-secret";
        let token = admin_token_for(secret);

        assert!(authorize_admin(Some(secret), Some(&token)).is_ok());

        // Wrong token, missing token, and — fail-closed — no configured secret: all rejected.
        assert_eq!(
            authorize_admin(Some(secret), Some("not-the-token")),
            Err("invalid admin_token")
        );
        assert_eq!(authorize_admin(Some(secret), None), Err("missing admin_token"));
        assert!(authorize_admin(None, Some(&token)).is_err());
        assert_eq!(authorize_admin(Some(""), Some(&token)), Err("invalid admin_token"));

        // Tokens are secret-specific.
        let other = admin_token_for("another-secret");
        assert_ne!(token, other);
        assert_eq!(authorize_admin(Some(secret), Some(&other)), Err("invalid admin_token"));
    }

    #[test]
    fn admin_token_is_lowercase_hex_hmac_sha256() {
        let token = admin_token_for("secret");
        assert_eq!(token.len(), 64, "HMAC-SHA256 hex must be 64 chars: {token}");
        assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));

        // Verification accepts uppercase hex (checked decoding is case-insensitive).
        assert!(verify_admin_token("secret", &token.to_ascii_uppercase()));
        assert!(!verify_admin_token("secret", "not-hex"));
    }

    #[test]
    fn admin_tokens_work_with_long_secrets() {
        let long_secret: String = (0..1024).map(|i| (b'a' + (i % 26) as u8) as char).collect();
        let token = admin_token_for(&long_secret);
        assert!(authorize_admin(Some(&long_secret), Some(&token)).is_ok());
    }

    #[test]
    fn extract_admin_token_reads_the_field_leniently() {
        assert_eq!(
            extract_admin_token(&serde_json::json!({ "admin_token": "tok" })),
            Some("tok".to_string())
        );
        assert_eq!(
            extract_admin_token(&serde_json::json!({ "admin_token": "tok", "game_id": "x" })),
            Some("tok".to_string())
        );
        assert_eq!(extract_admin_token(&serde_json::json!({})), None);
        assert_eq!(extract_admin_token(&serde_json::json!({ "admin_token": 42 })), None);
        assert_eq!(extract_admin_token(&serde_json::json!([1, 2])), None);
        assert_eq!(extract_admin_token(&serde_json::Value::Null), None);
    }

    #[test]
    fn status_names_match_the_wire_format() {
        assert_eq!(status_str(&SessionStatus::Starting), "starting");
        assert_eq!(status_str(&SessionStatus::Running), "running");
        assert_eq!(status_str(&SessionStatus::Exited(1)), "exited");
        assert_eq!(status_str(&SessionStatus::Stopped), "stopped");
    }

    #[test]
    fn request_validation_catches_missing_mistyped_and_unknown_fields() {
        assert!(serde_json::from_value::<GameIdRequest>(serde_json::json!({ "game_id": "abc" })).is_ok());
        assert!(serde_json::from_value::<GameIdRequest>(serde_json::json!({ "game_id": "abc", "admin_token": "tok" })).is_ok());
        assert!(serde_json::from_value::<GameIdRequest>(serde_json::json!({})).is_err());
        assert!(serde_json::from_value::<GameIdRequest>(serde_json::json!({ "game_id": 42 })).is_err());
        assert!(serde_json::from_value::<GameIdRequest>(serde_json::json!({ "game_id": "abc", "extra": 1 })).is_err());
        assert!(serde_json::from_value::<GameIdRequest>(serde_json::json!([1, 2, 3])).is_err());

        let start = serde_json::from_value::<StartSessionRequest>(serde_json::json!({
            "port": 32000,
            "world": 3,
            "min_players": 4,
            "admin_token": "tok",
        }))
        .unwrap();
        assert_eq!(start.port, Some(32000));
        assert_eq!(start.world, Some(3));
        assert_eq!(start.min_players, Some(4));
        assert_eq!(start.name, None);
        assert_eq!(start.admin_token.as_deref(), Some("tok"));
        assert!(serde_json::from_value::<StartSessionRequest>(serde_json::json!({ "foo": 1 })).is_err());
    }
}

// Create the plugin using our macro - zero unsafe code!
create_simple_plugin!(MaxAdminPlugin);
