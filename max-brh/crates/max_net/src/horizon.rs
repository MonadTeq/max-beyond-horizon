//! Horizon lobby/admin client over gRPC (`max-brh-docs/protocol-grpc.md`).
//!
//! The schemas in `max-brh-server/proto` are the single source of truth;
//! `build.rs` generates `horizon.v1` into `OUT_DIR` and [`proto`] exposes it.
//! This module is the typed, async façade used by the client:
//!
//! * [`HorizonClient::connect`] — HTTP/2 channel with a connect/request
//!   timeout;
//! * [`HorizonClient::hello`] — protocol handshake (WS `hello`);
//! * [`HorizonClient::list_games`] — lobby snapshot (WS `game_list`);
//! * [`HorizonClient::create_game`] — start a session, get its [`Ticket`];
//! * [`HorizonClient::join_game`] — get `host`/`port`/`ticket` for ENet;
//! * [`HorizonClient::ping`] — liveness probe.
//!
//! Errors are the standard gRPC statuses (`max-brh-docs/protocol-grpc.md` §4.2): the
//! legacy WS `error` text is carried in `status.message` and the category in
//! `status.code`, preserved by [`HorizonError::Status`]. There is no
//! `max_core` status type for lobby sessions, so [`GameStatus`] mirrors the
//! `GameStatus` proto enum and keeps the WS string mapping (§4.5) for parity.

use std::time::Duration;

use tonic::transport::{Channel, Endpoint};

/// Horizon protocol version (`PROTOCOL_VERSION = 1`, `Hello.protocol_version`).
pub const PROTOCOL_VERSION: u16 = 1;
/// Default per-call and connect timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
/// Default client identifier sent in `Hello.client`.
pub const CLIENT_NAME: &str = "max_net/0.1";

/// Generated `horizon.v1` messages, client and server stubs.
///
/// The generated module also contains the raw `HorizonClient`/`HorizonServer`
/// types, so tooling can use RPCs this façade does not wrap yet.
pub mod proto {
    tonic::include_proto!("horizon.v1");
}

/// Result alias for Horizon calls.
pub type HorizonResult<T> = Result<T, HorizonError>;

/// Error of a Horizon call.
#[derive(Debug, thiserror::Error)]
pub enum HorizonError {
    /// HTTP/2 transport failure (connect, TLS, GOAWAY...).
    #[error("Horizon transport error: {0}")]
    Transport(#[from] tonic::transport::Error),
    /// A gRPC status returned by the server.
    #[error("Horizon gRPC status {code:?}: {message}")]
    Status {
        /// Standard gRPC code.
        code: tonic::Code,
        /// Exact legacy WS `error` text.
        message: String,
    },
    /// The call did not finish within the configured timeout.
    #[error("Horizon {operation} timed out after {timeout:?}")]
    Timeout {
        /// Operation name.
        operation: &'static str,
        /// Configured timeout.
        timeout: Duration,
    },
    /// The endpoint URI is invalid.
    #[error("invalid Horizon endpoint: {0}")]
    Endpoint(String),
    /// The server returned a response that violates the schema invariants
    /// (e.g. a `uint32` port above 65535).
    #[error("invalid Horizon response: {0}")]
    InvalidResponse(String),
}

impl From<tonic::Status> for HorizonError {
    fn from(status: tonic::Status) -> Self {
        Self::Status {
            code: status.code(),
            message: status.message().to_string(),
        }
    }
}

impl HorizonError {
    /// gRPC code, when the failure arrived as a status.
    pub const fn code(&self) -> Option<tonic::Code> {
        match self {
            Self::Status { code, .. } => Some(*code),
            _ => None,
        }
    }

    /// True for `NOT_FOUND` (unknown game id, `max-brh-docs/protocol-grpc.md` §6.1).
    pub const fn is_not_found(&self) -> bool {
        matches!(self.code(), Some(tonic::Code::NotFound))
    }
}

/// Session lifecycle state (`GameStatus` in `types.proto`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameStatus {
    /// Proto3 zero value; the server never sends it.
    Unspecified,
    /// The headless process is being spawned.
    Starting,
    /// Running and accepting players.
    Running,
    /// The process exited on its own.
    Exited,
    /// Stopped through the session manager.
    Stopped,
    /// Unknown to the manager (admin status only).
    Unknown,
}

impl GameStatus {
    /// Maps the proto enum value.
    pub fn from_proto(value: i32) -> Self {
        match proto::GameStatus::try_from(value) {
            Ok(proto::GameStatus::Unspecified) | Err(_) => Self::Unspecified,
            Ok(proto::GameStatus::Starting) => Self::Starting,
            Ok(proto::GameStatus::Running) => Self::Running,
            Ok(proto::GameStatus::Exited) => Self::Exited,
            Ok(proto::GameStatus::Stopped) => Self::Stopped,
            Ok(proto::GameStatus::Unknown) => Self::Unknown,
        }
    }

    /// Proto enum value.
    pub const fn as_proto(self) -> proto::GameStatus {
        match self {
            Self::Unspecified => proto::GameStatus::Unspecified,
            Self::Starting => proto::GameStatus::Starting,
            Self::Running => proto::GameStatus::Running,
            Self::Exited => proto::GameStatus::Exited,
            Self::Stopped => proto::GameStatus::Stopped,
            Self::Unknown => proto::GameStatus::Unknown,
        }
    }

    /// Legacy WS `status` string (`max-brh-docs/protocol-grpc.md` §4.5).
    pub const fn as_ws_str(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Exited => "exited",
            Self::Stopped => "stopped",
            Self::Unknown => "unknown",
        }
    }

    /// Parses the legacy WS `status` string.
    pub fn from_ws_str(value: &str) -> Self {
        match value {
            "starting" => Self::Starting,
            "running" => Self::Running,
            "exited" => Self::Exited,
            "stopped" => Self::Stopped,
            "unknown" => Self::Unknown,
            _ => Self::Unspecified,
        }
    }
}

/// One lobby game (`GameInfo` in `types.proto`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInfo {
    /// Session UUID in lowercase hyphenated form.
    pub id: String,
    /// Human-readable game name.
    pub name: String,
    /// World index (`0..=23`).
    pub world: i64,
    /// Lifecycle state.
    pub status: GameStatus,
    /// UDP port of the headless process.
    pub port: u16,
    /// Players currently registered.
    pub players: u32,
    /// Minimum players before the session starts.
    pub min_players: u32,
    /// Password protected game.
    pub password_protected: bool,
}

impl GameInfo {
    /// Bridges to the legacy WS JSON [`crate::protocol::GameListing`] shape.
    ///
    /// The legacy `world` field is a `u8` while the proto carries an `int64`;
    /// out-of-range values are clamped to 0 (they cannot be rendered by the
    /// legacy UI anyway).
    pub fn to_legacy_listing(&self) -> crate::protocol::GameListing {
        crate::protocol::GameListing {
            id: self.id.clone(),
            name: self.name.clone(),
            world: u8::try_from(self.world).unwrap_or(0),
            status: self.status.as_ws_str().to_string(),
            port: self.port,
            players: self.players,
            min_players: self.min_players,
            password_protected: self.password_protected,
        }
    }
}

impl TryFrom<proto::GameInfo> for GameInfo {
    type Error = HorizonError;

    fn try_from(value: proto::GameInfo) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            name: value.name,
            world: value.world,
            status: GameStatus::from_proto(value.status),
            port: port_from_u32(value.port)?,
            players: value.players,
            min_players: value.min_players,
            password_protected: value.password_protected,
        })
    }
}

/// Ticket format (`TicketKind` in `types.proto`, `max-brh-docs/protocol.md` §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TicketKind {
    /// Proto3 zero value.
    Unspecified,
    /// Password-as-ticket (`""` for open games).
    Legacy,
    /// `v1.<session_id>.<player_id>.<unix_expiry>.<hex_hmac>`.
    Signed,
}

impl TicketKind {
    fn from_proto(value: i32) -> Self {
        match proto::TicketKind::try_from(value) {
            Ok(proto::TicketKind::Legacy) => Self::Legacy,
            Ok(proto::TicketKind::Signed) => Self::Signed,
            Ok(proto::TicketKind::Unspecified) | Err(_) => Self::Unspecified,
        }
    }
}

/// Session ticket handed to the ENet handshake (`TP 00`, `ticket` field).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    /// Exact string to send in the handshake.
    pub value: String,
    /// Which format [`Self::value`] carries.
    pub kind: TicketKind,
    /// Parsed `session_id` of a signed ticket.
    pub session_id: Option<String>,
    /// Parsed `player_id` of a signed ticket.
    pub player_id: Option<String>,
    /// Expiration in whole Unix seconds.
    pub expires_at_unix: Option<i64>,
}

impl Ticket {
    /// True for a signed `v1.*` ticket.
    pub const fn is_signed(&self) -> bool {
        matches!(self.kind, TicketKind::Signed)
    }

    /// True when `now` is at or past the signed ticket's expiry.
    pub fn is_expired_at(&self, now_unix: i64) -> bool {
        self.expires_at_unix
            .is_some_and(|expiry| now_unix >= expiry)
    }
}

impl From<proto::Ticket> for Ticket {
    fn from(value: proto::Ticket) -> Self {
        Self {
            value: value.value,
            kind: TicketKind::from_proto(value.kind),
            session_id: value.session_id,
            player_id: value.player_id,
            expires_at_unix: value.expires_at_unix,
        }
    }
}

/// Successful `JoinGame` result: everything the UDP session needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameJoined {
    /// Session id (echo of the request).
    pub game_id: String,
    /// Host the ENet client connects to.
    pub host: String,
    /// UDP port of the headless process.
    pub port: u16,
    /// Ticket to present in `TP 00`.
    pub ticket: Ticket,
}

/// `Hello` acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelloAck {
    /// Protocol version the server speaks.
    pub protocol_version: u16,
    /// Always `"protobuf"` on gRPC.
    pub format: String,
    /// Compression negotiated by gRPC (`"none"`, `"gzip"`, ...).
    pub compression: String,
    /// Always unset over gRPC.
    pub dictionary_id: Option<u32>,
}

impl TryFrom<proto::HelloAck> for HelloAck {
    type Error = HorizonError;

    fn try_from(value: proto::HelloAck) -> Result<Self, Self::Error> {
        Ok(Self {
            protocol_version: u16::try_from(value.protocol_version).map_err(|_| {
                HorizonError::InvalidResponse(format!(
                    "HelloAck.protocol_version {} does not fit u16",
                    value.protocol_version
                ))
            })?,
            format: value.format,
            compression: value.compression,
            dictionary_id: value.dictionary_id,
        })
    }
}

/// Parameters of `CreateGame`; every field is optional and unset fields keep
/// the Horizon session defaults (`max-brh-docs/protocol-grpc.md` §5.3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreateGameParams {
    /// Human-readable game name.
    pub name: Option<String>,
    /// World index (`0..=23`).
    pub world: Option<i64>,
    /// Seconds per turn.
    pub timer: Option<i64>,
    /// End-turn timer in seconds.
    pub endturn: Option<i64>,
    /// Play-mode value passed through to the server (the proto comment and the
    /// `max_core::enums::PlayMode` representation disagree; keep the raw value
    /// so no silent remapping happens).
    pub play_mode: Option<i64>,
    /// Starting credits.
    pub start_gold: Option<i64>,
    /// Victory type.
    pub victory_type: Option<i64>,
    /// Victory limit.
    pub victory_limit: Option<i64>,
    /// Minimum players (`>= 2`).
    pub min_players: Option<u32>,
    /// Password (doubles as the legacy ticket for protected games).
    pub password: Option<String>,
}

impl From<CreateGameParams> for proto::CreateGameRequest {
    fn from(value: CreateGameParams) -> Self {
        Self {
            name: value.name,
            world: value.world,
            timer: value.timer,
            endturn: value.endturn,
            play_mode: value.play_mode,
            start_gold: value.start_gold,
            victory_type: value.victory_type,
            victory_limit: value.victory_limit,
            min_players: value.min_players,
            password: value.password,
        }
    }
}

/// Result of `CreateGame`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateGameResult {
    /// The created game.
    pub game: GameInfo,
    /// Ticket for the creator.
    pub ticket: Ticket,
}

/// Result of `Ping`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingResult {
    /// Server wall-clock milliseconds since the Unix epoch.
    pub timestamp: u64,
    /// Always `"pong"`.
    pub message: String,
}

/// Async Horizon gRPC client.
#[derive(Debug, Clone)]
pub struct HorizonClient {
    inner: proto::horizon_client::HorizonClient<Channel>,
    timeout: Duration,
    endpoint: String,
}

impl HorizonClient {
    /// Connects to `http://host:port` with [`DEFAULT_TIMEOUT`].
    pub async fn connect(host: &str, port: u16) -> HorizonResult<Self> {
        Self::connect_with_timeout(host, port, DEFAULT_TIMEOUT).await
    }

    /// Connects with an explicit connect/request timeout.
    pub async fn connect_with_timeout(
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> HorizonResult<Self> {
        let endpoint = format!("http://{host}:{port}");
        let channel = Endpoint::from_shared(endpoint.clone())
            .map_err(|error| HorizonError::Endpoint(error.to_string()))?
            .connect_timeout(timeout)
            .timeout(timeout)
            .connect()
            .await?;
        Ok(Self {
            inner: proto::horizon_client::HorizonClient::new(channel),
            timeout,
            endpoint,
        })
    }

    /// Wraps an already-established channel (used by tests/tooling).
    pub fn with_channel(channel: Channel, timeout: Duration) -> Self {
        Self {
            inner: proto::horizon_client::HorizonClient::new(channel),
            timeout,
            endpoint: "channel".to_string(),
        }
    }

    /// Endpoint URI this client was built for.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Current per-call timeout.
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Replaces the per-call timeout.
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    /// Optional `Hello` handshake (WS `hello`).
    pub async fn hello(&mut self, protocol_version: u16, client: &str) -> HorizonResult<HelloAck> {
        let request = proto::HelloRequest {
            protocol_version: u32::from(protocol_version),
            client: client.to_string(),
            dictionary_id: None,
        };
        let response = Self::call(self.timeout, "hello", self.inner.hello(request))
            .await?
            .into_inner();
        HelloAck::try_from(response)
    }

    /// Snapshot of the lobby games (WS `game_list`).
    pub async fn list_games(&mut self) -> HorizonResult<Vec<GameInfo>> {
        let response = Self::call(
            self.timeout,
            "list_games",
            self.inner.list_games(proto::ListGamesRequest {}),
        )
        .await?
        .into_inner();
        response.games.into_iter().map(GameInfo::try_from).collect()
    }

    /// Starts a headless session and returns its game info and ticket
    /// (WS `game_created`).
    pub async fn create_game(
        &mut self,
        params: CreateGameParams,
    ) -> HorizonResult<CreateGameResult> {
        let request: proto::CreateGameRequest = params.into();
        let response = Self::call(self.timeout, "create_game", self.inner.create_game(request))
            .await?
            .into_inner();
        let game = response.game.ok_or_else(|| {
            HorizonError::InvalidResponse("CreateGameResponse.game is unset".into())
        })?;
        let ticket = response.ticket.ok_or_else(|| {
            HorizonError::InvalidResponse("CreateGameResponse.ticket is unset".into())
        })?;
        Ok(CreateGameResult {
            game: GameInfo::try_from(game)?,
            ticket: ticket.into(),
        })
    }

    /// Joins a game and receives `host`/`port`/`ticket` for the ENet session
    /// (WS `game_joined`).
    pub async fn join_game(
        &mut self,
        game_id: &str,
        password: Option<&str>,
    ) -> HorizonResult<GameJoined> {
        let request = proto::JoinGameRequest {
            game_id: game_id.to_string(),
            password: password.map(str::to_string),
        };
        let response = Self::call(self.timeout, "join_game", self.inner.join_game(request))
            .await?
            .into_inner();
        let port = port_from_u32(response.port)?;
        let ticket = response.ticket.ok_or_else(|| {
            HorizonError::InvalidResponse("JoinGameResponse.ticket is unset".into())
        })?;
        if response.host.is_empty() {
            return Err(HorizonError::InvalidResponse(
                "JoinGameResponse.host is empty".into(),
            ));
        }
        Ok(GameJoined {
            game_id: response.game_id,
            host: response.host,
            port,
            ticket: ticket.into(),
        })
    }

    /// Liveness probe (WS `system:ping`).
    pub async fn ping(&mut self, payload: Option<Vec<u8>>) -> HorizonResult<PingResult> {
        let request = proto::PingRequest { payload };
        let response = Self::call(self.timeout, "ping", self.inner.ping(request))
            .await?
            .into_inner();
        Ok(PingResult {
            timestamp: response.timestamp,
            message: response.message,
        })
    }

    /// Applies the per-call timeout and maps transport/status errors.
    ///
    /// A free associated function (not a method) so the caller can borrow
    /// `self.inner` mutably for the RPC future while reading `self.timeout`.
    async fn call<T>(
        timeout: Duration,
        operation: &'static str,
        future: impl std::future::Future<Output = Result<tonic::Response<T>, tonic::Status>>,
    ) -> HorizonResult<tonic::Response<T>> {
        match tokio::time::timeout(timeout, future).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(status)) => Err(status.into()),
            Err(_) => Err(HorizonError::Timeout { operation, timeout }),
        }
    }
}

fn port_from_u32(value: u32) -> HorizonResult<u16> {
    u16::try_from(value)
        .map_err(|_| HorizonError::InvalidResponse(format!("port {value} does not fit u16")))
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{Request, Response as TonicResponse, Status};

    use proto::horizon_server::{Horizon as HorizonService, HorizonServer};

    #[derive(Default)]
    struct MockHorizon;

    fn game(id: &str, status: proto::GameStatus) -> proto::GameInfo {
        proto::GameInfo {
            id: id.to_string(),
            name: "Friday 2v2".to_string(),
            world: 3,
            status: status as i32,
            port: 31554,
            players: 2,
            min_players: 4,
            password_protected: false,
        }
    }

    #[tonic::async_trait]
    impl HorizonService for MockHorizon {
        async fn hello(
            &self,
            request: Request<proto::HelloRequest>,
        ) -> Result<TonicResponse<proto::HelloAck>, Status> {
            let request = request.into_inner();
            if request.protocol_version != u32::from(PROTOCOL_VERSION) {
                return Err(Status::failed_precondition(
                    "protocol version not supported",
                ));
            }
            Ok(TonicResponse::new(proto::HelloAck {
                protocol_version: request.protocol_version,
                format: "protobuf".into(),
                compression: "none".into(),
                dictionary_id: None,
            }))
        }

        async fn list_games(
            &self,
            _request: Request<proto::ListGamesRequest>,
        ) -> Result<TonicResponse<proto::ListGamesResponse>, Status> {
            Ok(TonicResponse::new(proto::ListGamesResponse {
                games: vec![game("game-1", proto::GameStatus::Running)],
            }))
        }

        async fn create_game(
            &self,
            request: Request<proto::CreateGameRequest>,
        ) -> Result<TonicResponse<proto::CreateGameResponse>, Status> {
            let request = request.into_inner();
            if request.min_players == Some(1) {
                return Err(Status::invalid_argument(
                    "a network game needs at least 2 players",
                ));
            }
            Ok(TonicResponse::new(proto::CreateGameResponse {
                game: Some(proto::GameInfo {
                    name: request.name.unwrap_or_else(|| "created".into()),
                    password_protected: request.password.is_some(),
                    ..game("created-game", proto::GameStatus::Starting)
                }),
                ticket: Some(proto::Ticket {
                    value: request.password.unwrap_or_default(),
                    kind: proto::TicketKind::Legacy as i32,
                    session_id: None,
                    player_id: None,
                    expires_at_unix: None,
                }),
            }))
        }

        async fn join_game(
            &self,
            request: Request<proto::JoinGameRequest>,
        ) -> Result<TonicResponse<proto::JoinGameResponse>, Status> {
            let request = request.into_inner();
            if request.game_id == "missing" {
                return Err(Status::not_found("game not found"));
            }
            if request.game_id == "protected" && request.password.as_deref() != Some("sesame") {
                return Err(Status::permission_denied("invalid password"));
            }
            Ok(TonicResponse::new(proto::JoinGameResponse {
                game_id: request.game_id.clone(),
                host: "127.0.0.1".into(),
                port: 31554,
                ticket: Some(proto::Ticket {
                    value: "v1.session.player.1772341200.0a1b".into(),
                    kind: proto::TicketKind::Signed as i32,
                    session_id: Some("session".into()),
                    player_id: Some("player".into()),
                    expires_at_unix: Some(1_772_341_200),
                }),
            }))
        }

        async fn ping(
            &self,
            request: Request<proto::PingRequest>,
        ) -> Result<TonicResponse<proto::PingResponse>, Status> {
            if request.get_ref().payload.as_deref() == Some(&b"slow"[..]) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Ok(TonicResponse::new(proto::PingResponse {
                timestamp: 1_700_000_000_000,
                message: "pong".into(),
            }))
        }

        // Admin RPCs are not exercised by this crate's client façade; the
        // generated server still requires them.
        async fn admin_list_sessions(
            &self,
            _request: Request<proto::AdminListSessionsRequest>,
        ) -> Result<TonicResponse<proto::AdminListSessionsResponse>, Status> {
            Err(Status::unimplemented("admin API is not part of max_net"))
        }

        async fn admin_session_status(
            &self,
            _request: Request<proto::AdminSessionStatusRequest>,
        ) -> Result<TonicResponse<proto::AdminSessionStatusResponse>, Status> {
            Err(Status::unimplemented("admin API is not part of max_net"))
        }

        async fn admin_session_stop(
            &self,
            _request: Request<proto::AdminSessionStopRequest>,
        ) -> Result<TonicResponse<proto::AdminSessionStopResponse>, Status> {
            Err(Status::unimplemented("admin API is not part of max_net"))
        }

        async fn admin_session_restart(
            &self,
            _request: Request<proto::AdminSessionRestartRequest>,
        ) -> Result<TonicResponse<proto::AdminSessionRestartResponse>, Status> {
            Err(Status::unimplemented("admin API is not part of max_net"))
        }

        async fn admin_session_start(
            &self,
            _request: Request<proto::AdminSessionStartRequest>,
        ) -> Result<TonicResponse<proto::AdminSessionStartResponse>, Status> {
            Err(Status::unimplemented("admin API is not part of max_net"))
        }
    }

    /// Starts the mock server on `127.0.0.1:0` and returns its port + task.
    async fn start_mock() -> (u16, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock Horizon");
        let port = listener.local_addr().expect("local_addr").port();
        let incoming = TcpListenerStream::new(listener);
        let handle = tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(HorizonServer::new(MockHorizon))
                .serve_with_incoming(incoming)
                .await;
        });
        (port, handle)
    }

    #[tokio::test]
    async fn hello_list_create_join_ping_round_trip() {
        let (port, handle) = start_mock().await;
        let mut client =
            HorizonClient::connect_with_timeout("127.0.0.1", port, Duration::from_secs(5))
                .await
                .expect("connect mock Horizon");

        let ack = client
            .hello(PROTOCOL_VERSION, CLIENT_NAME)
            .await
            .expect("hello");
        assert_eq!(ack.protocol_version, PROTOCOL_VERSION);
        assert_eq!(ack.format, "protobuf");
        assert_eq!(ack.compression, "none");

        let games = client.list_games().await.expect("list");
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].port, 31554);
        assert_eq!(games[0].status, GameStatus::Running);
        assert_eq!(games[0].status.as_ws_str(), "running");

        let created = client
            .create_game(CreateGameParams {
                name: Some("2v2".into()),
                world: Some(3),
                password: Some("sesame".into()),
                ..CreateGameParams::default()
            })
            .await
            .expect("create");
        assert_eq!(created.game.name, "2v2");
        assert!(created.game.password_protected);
        assert_eq!(created.ticket.kind, TicketKind::Legacy);
        assert_eq!(created.ticket.value, "sesame");

        let joined = client.join_game("game-1", None).await.expect("join");
        assert_eq!(joined.game_id, "game-1");
        assert_eq!(joined.host, "127.0.0.1");
        assert_eq!(joined.port, 31554);
        assert!(joined.ticket.is_signed());
        assert_eq!(joined.ticket.session_id.as_deref(), Some("session"));
        assert!(!joined.ticket.is_expired_at(1_700_000_000));

        let pong = client.ping(Some(b"hi".to_vec())).await.expect("ping");
        assert_eq!(pong.message, "pong");
        assert_eq!(pong.timestamp, 1_700_000_000_000);

        handle.abort();
    }

    #[tokio::test]
    async fn statuses_are_mapped_to_typed_errors() {
        let (port, handle) = start_mock().await;
        let mut client =
            HorizonClient::connect_with_timeout("127.0.0.1", port, Duration::from_secs(5))
                .await
                .expect("connect mock Horizon");

        let error = client.join_game("missing", None).await.unwrap_err();
        assert_eq!(error.code(), Some(tonic::Code::NotFound));
        assert!(error.is_not_found());
        assert!(error.to_string().contains("game not found"));

        let error = client
            .join_game("protected", Some("wrong"))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(tonic::Code::PermissionDenied));

        let error = client
            .create_game(CreateGameParams {
                min_players: Some(1),
                ..CreateGameParams::default()
            })
            .await
            .unwrap_err();
        assert_eq!(error.code(), Some(tonic::Code::InvalidArgument));

        handle.abort();
    }

    #[tokio::test]
    async fn per_call_timeout_is_enforced() {
        let (port, handle) = start_mock().await;
        let mut client =
            HorizonClient::connect_with_timeout("127.0.0.1", port, Duration::from_secs(5))
                .await
                .expect("connect mock Horizon");
        client.set_timeout(Duration::from_millis(10));

        let error = client.ping(Some(b"slow".to_vec())).await.unwrap_err();
        assert!(matches!(
            error,
            HorizonError::Timeout {
                operation: "ping",
                ..
            }
        ));

        handle.abort();
    }

    #[test]
    fn status_and_ticket_mappings() {
        assert_eq!(GameStatus::from_proto(0), GameStatus::Unspecified);
        assert_eq!(GameStatus::from_proto(1), GameStatus::Starting);
        assert_eq!(GameStatus::from_proto(2), GameStatus::Running);
        assert_eq!(GameStatus::from_proto(3), GameStatus::Exited);
        assert_eq!(GameStatus::from_proto(4), GameStatus::Stopped);
        assert_eq!(GameStatus::from_proto(5), GameStatus::Unknown);
        assert_eq!(GameStatus::from_proto(99), GameStatus::Unspecified);
        assert_eq!(GameStatus::Running.as_proto(), proto::GameStatus::Running);
        assert_eq!(GameStatus::Running.as_ws_str(), "running");
        assert_eq!(GameStatus::from_ws_str("running"), GameStatus::Running);
        assert_eq!(GameStatus::from_ws_str("bogus"), GameStatus::Unspecified);

        let info = GameInfo {
            id: "id".into(),
            name: "name".into(),
            world: 3,
            status: GameStatus::Running,
            port: 31554,
            players: 2,
            min_players: 4,
            password_protected: false,
        };
        let listing = info.to_legacy_listing();
        assert_eq!(listing.world, 3);
        assert_eq!(listing.status, "running");
        assert_eq!(listing.port, 31554);

        let ticket = Ticket {
            value: "v1.a.b.10.ff".into(),
            kind: TicketKind::Signed,
            session_id: Some("a".into()),
            player_id: Some("b".into()),
            expires_at_unix: Some(10),
        };
        assert!(ticket.is_signed());
        assert!(ticket.is_expired_at(10));
        assert!(!ticket.is_expired_at(9));
    }

    #[test]
    fn port_validation_rejects_values_above_u16() {
        assert_eq!(port_from_u32(31554).unwrap(), 31554);
        assert!(matches!(
            port_from_u32(70_000),
            Err(HorizonError::InvalidResponse(_))
        ));
    }
}
