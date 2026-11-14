//! The `horizon.v1.Horizon` service: a thin protobuf shell around
//! [`HorizonBackend`].
//!
//! Each RPC is one WS request/response pair (`docs/protocol-grpc.md` §3):
//!
//! | RPC                  | WS event                     |
//! |----------------------|------------------------------|
//! | `Hello`              | `hello` → `hello_ack`        |
//! | `ListGames`          | `max_lobby:list_games`       |
//! | `CreateGame`         | `max_lobby:create_game`      |
//! | `JoinGame`           | `max_lobby:join_game`        |
//! | `AdminListSessions`  | `max_admin:list_sessions`    |
//! | `AdminSessionStatus` | `max_admin:session_status`   |
//! | `AdminSessionStop`   | `max_admin:stop_session`     |
//! | `AdminSessionRestart`| `max_admin:restart_session`  |
//! | `AdminSessionStart`  | `max_admin:start_session`    |
//! | `Ping`               | `system:ping`                |
//!
//! The layer does three things and nothing else: convert protobuf to the
//! backend's domain types, convert results back to protobuf, and turn
//! [`ServiceError`] into a `tonic::Status` whose message is the verbatim legacy
//! WS text (`docs/protocol-grpc.md` §6).
//!
//! gRPC has no per-connection identity (every RPC is independent), so a fresh
//! player UUID is minted per `CreateGame`/`JoinGame` call — the same thing the
//! WS server does for every new connection, and what the signed ticket binds to.

use std::sync::Arc;

use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::backend::{
    AdminStartParams, CreateGameParams, GameInfo, GameOptions, GameStatus, HorizonBackend,
    TicketInfo, TicketKind,
};
use crate::proto::v1::{self, horizon_server::Horizon};

/// Horizon protocol version (`Hello.protocol_version`, WS `PROTOCOL_VERSION = 1`).
pub const PROTOCOL_VERSION: u16 = 1;

/// The gRPC implementation of `horizon.v1.Horizon`.
pub struct HorizonService {
    backend: Arc<dyn HorizonBackend>,
}

impl HorizonService {
    /// Creates the service over a backend.
    pub fn new(backend: Arc<dyn HorizonBackend>) -> Self {
        Self { backend }
    }
}

#[tonic::async_trait]
impl Horizon for HorizonService {
    async fn hello(
        &self,
        request: Request<v1::HelloRequest>,
    ) -> Result<Response<v1::HelloAck>, Status> {
        let request = request.into_inner();

        // `protocol_version` is a u16 on the wire (JSON) and a uint32 in
        // protobuf; values that do not fit a u16 are invalid input.
        let protocol_version = u16::try_from(request.protocol_version).map_err(|_| {
            Status::invalid_argument(format!(
                "protocol_version {} does not fit u16",
                request.protocol_version
            ))
        })?;

        // WS close 4000 → FAILED_PRECONDITION (`docs/protocol-grpc.md` §6.2).
        if protocol_version != PROTOCOL_VERSION {
            return Err(Status::failed_precondition("protocol version not supported"));
        }

        Ok(Response::new(v1::HelloAck {
            protocol_version: u32::from(protocol_version),
            // Always "protobuf" on this transport (§4.5).
            format: "protobuf".to_string(),
            // The listener negotiates no message compression, so every message
            // is sent uncompressed. (gRPC does not expose the negotiated codec
            // to the service; enabling gzip/zstd here would change this value.)
            compression: "none".to_string(),
            // Compression-dictionary is a WS-only concept: always unset.
            dictionary_id: None,
        }))
    }

    async fn list_games(
        &self,
        _request: Request<v1::ListGamesRequest>,
    ) -> Result<Response<v1::ListGamesResponse>, Status> {
        let games = self.backend.list_games().await.map_err(Status::from)?;
        Ok(Response::new(v1::ListGamesResponse {
            games: games.into_iter().map(game_info_to_proto).collect(),
        }))
    }

    async fn create_game(
        &self,
        request: Request<v1::CreateGameRequest>,
    ) -> Result<Response<v1::CreateGameResponse>, Status> {
        let request = request.into_inner();
        let password = request.password.clone();
        let params = CreateGameParams {
            game: game_options_from_proto(request),
            password,
        };

        let created = self
            .backend
            .create_game(Uuid::new_v4(), params)
            .await
            .map_err(Status::from)?;

        Ok(Response::new(v1::CreateGameResponse {
            game: Some(game_info_to_proto(created.game)),
            ticket: Some(ticket_to_proto(created.ticket)),
        }))
    }

    async fn join_game(
        &self,
        request: Request<v1::JoinGameRequest>,
    ) -> Result<Response<v1::JoinGameResponse>, Status> {
        let request = request.into_inner();

        let joined = self
            .backend
            .join_game(Uuid::new_v4(), &request.game_id, request.password)
            .await
            .map_err(Status::from)?;

        Ok(Response::new(v1::JoinGameResponse {
            game_id: joined.game_id.to_string(),
            host: joined.host,
            port: u32::from(joined.port),
            ticket: Some(ticket_to_proto(joined.ticket)),
        }))
    }

    async fn admin_list_sessions(
        &self,
        request: Request<v1::AdminListSessionsRequest>,
    ) -> Result<Response<v1::AdminListSessionsResponse>, Status> {
        let sessions = self
            .backend
            .admin_list_sessions(request.into_inner().admin_token)
            .await
            .map_err(Status::from)?;

        Ok(Response::new(v1::AdminListSessionsResponse {
            sessions: sessions
                .into_iter()
                .map(|session| v1::AdminSession {
                    game_id: session.game_id.to_string(),
                    name: session.name,
                    port: u32::from(session.port),
                    state: status_to_proto(session.state),
                    dir: session.dir,
                })
                .collect(),
        }))
    }

    async fn admin_session_status(
        &self,
        request: Request<v1::AdminSessionStatusRequest>,
    ) -> Result<Response<v1::AdminSessionStatusResponse>, Status> {
        let request = request.into_inner();

        let state = self
            .backend
            .admin_session_status(request.admin_token, &request.game_id)
            .await
            .map_err(Status::from)?;

        Ok(Response::new(v1::AdminSessionStatusResponse {
            game_id: request.game_id,
            status: Some(v1::SessionStatusInfo {
                state: status_to_proto(state),
            }),
        }))
    }

    async fn admin_session_stop(
        &self,
        request: Request<v1::AdminSessionStopRequest>,
    ) -> Result<Response<v1::AdminSessionStopResponse>, Status> {
        let request = request.into_inner();

        let outcome = self
            .backend
            .admin_session_stop(request.admin_token, &request.game_id)
            .await
            .map_err(Status::from)?;

        Ok(Response::new(v1::AdminSessionStopResponse {
            game_id: outcome.game_id,
            ok: outcome.ok,
            // WS parity: manager failures are inline (`ok:false` + `error`),
            // authorization/configuration failures are statuses.
            error: outcome.error.map(|error| error.to_proto_error()),
        }))
    }

    async fn admin_session_restart(
        &self,
        request: Request<v1::AdminSessionRestartRequest>,
    ) -> Result<Response<v1::AdminSessionRestartResponse>, Status> {
        let request = request.into_inner();

        let outcome = self
            .backend
            .admin_session_restart(request.admin_token, &request.game_id)
            .await
            .map_err(Status::from)?;

        Ok(Response::new(v1::AdminSessionRestartResponse {
            game_id: outcome.game_id,
            ok: outcome.ok,
            new_game_id: outcome.new_game_id.map(|id| id.to_string()),
            port: outcome.port.map(u32::from),
            error: outcome.error.map(|error| error.to_proto_error()),
        }))
    }

    async fn admin_session_start(
        &self,
        request: Request<v1::AdminSessionStartRequest>,
    ) -> Result<Response<v1::AdminSessionStartResponse>, Status> {
        let request = request.into_inner();
        let admin_token = request.admin_token.clone();
        let params = AdminStartParams {
            port: request.port,
            game: game_options_from_proto(request),
        };

        let outcome = self
            .backend
            .admin_session_start(admin_token, params)
            .await
            .map_err(Status::from)?;

        Ok(Response::new(v1::AdminSessionStartResponse {
            game_id: outcome.game_id.map(|id| id.to_string()),
            port: outcome.port.map(u32::from),
            // The legacy success frame omits `ok`; the proto sets it explicitly.
            ok: outcome.ok,
            error: outcome.error.map(|error| error.to_proto_error()),
        }))
    }

    async fn ping(
        &self,
        _request: Request<v1::PingRequest>,
    ) -> Result<Response<v1::PingResponse>, Status> {
        // The WS server ignores the ping payload; so do we.
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or(0);

        Ok(Response::new(v1::PingResponse {
            timestamp,
            message: "pong".to_string(),
        }))
    }
}

// ============================================================================
// Protobuf ↔ domain conversion
// ============================================================================

/// Extracts the optional game fields shared by `CreateGame` and `AdminSessionStart`.
fn game_options_from_proto<T>(request: T) -> GameOptions
where
    T: Into<GameOptions>,
{
    request.into()
}

/// The `CreateGameRequest` fields that map 1:1 onto [`GameOptions`].
impl From<v1::CreateGameRequest> for GameOptions {
    fn from(request: v1::CreateGameRequest) -> Self {
        Self {
            name: request.name,
            world: request.world,
            timer: request.timer,
            endturn: request.endturn,
            play_mode: request.play_mode,
            start_gold: request.start_gold,
            victory_type: request.victory_type,
            victory_limit: request.victory_limit,
            min_players: request.min_players,
        }
    }
}

/// The `AdminSessionStartRequest` fields that map 1:1 onto [`GameOptions`].
impl From<v1::AdminSessionStartRequest> for GameOptions {
    fn from(request: v1::AdminSessionStartRequest) -> Self {
        Self {
            name: request.name,
            world: request.world,
            timer: request.timer,
            endturn: request.endturn,
            play_mode: request.play_mode,
            start_gold: request.start_gold,
            victory_type: request.victory_type,
            victory_limit: request.victory_limit,
            min_players: request.min_players,
        }
    }
}

/// Converts a lobby game to its protobuf form (`types.proto` `GameInfo`).
fn game_info_to_proto(game: GameInfo) -> v1::GameInfo {
    v1::GameInfo {
        id: game.id.to_string(),
        name: game.name,
        world: game.world,
        status: status_to_proto(game.status),
        port: u32::from(game.port),
        players: game.players,
        min_players: game.min_players,
        password_protected: game.password_protected,
    }
}

/// Converts a ticket, including the parsed fields of a signed one.
fn ticket_to_proto(ticket: TicketInfo) -> v1::Ticket {
    v1::Ticket {
        value: ticket.value,
        kind: match ticket.kind {
            TicketKind::Legacy => v1::TicketKind::Legacy as i32,
            TicketKind::Signed => v1::TicketKind::Signed as i32,
        },
        session_id: ticket.session_id.map(|id| id.to_string()),
        player_id: ticket.player_id.map(|id| id.to_string()),
        expires_at_unix: ticket.expires_at_unix,
    }
}

/// Converts a lifecycle state to the `GameStatus` enum value.
fn status_to_proto(status: GameStatus) -> i32 {
    let status = match status {
        GameStatus::Starting => v1::GameStatus::Starting,
        GameStatus::Running => v1::GameStatus::Running,
        GameStatus::Exited => v1::GameStatus::Exited,
        GameStatus::Stopped => v1::GameStatus::Stopped,
        GameStatus::Unknown => v1::GameStatus::Unknown,
    };
    status as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_to_the_proto_enum() {
        assert_eq!(status_to_proto(GameStatus::Starting), 1);
        assert_eq!(status_to_proto(GameStatus::Running), 2);
        assert_eq!(status_to_proto(GameStatus::Exited), 3);
        assert_eq!(status_to_proto(GameStatus::Stopped), 4);
        assert_eq!(status_to_proto(GameStatus::Unknown), 5);
        // The server never emits the proto3 zero value.
        assert_ne!(status_to_proto(GameStatus::Unknown), 0);
    }

    #[test]
    fn signed_tickets_keep_their_parsed_fields() {
        let session = Uuid::new_v4();
        let player = Uuid::new_v4();
        let proto = ticket_to_proto(TicketInfo {
            value: "v1.a.b.10.ff".to_string(),
            kind: TicketKind::Signed,
            session_id: Some(session),
            player_id: Some(player),
            expires_at_unix: Some(10),
        });

        assert_eq!(proto.kind, v1::TicketKind::Signed as i32);
        let session_id = session.to_string();
        let player_id = player.to_string();
        assert_eq!(proto.session_id.as_deref(), Some(session_id.as_str()));
        assert_eq!(proto.player_id.as_deref(), Some(player_id.as_str()));
        assert_eq!(proto.expires_at_unix, Some(10));
    }

    #[test]
    fn legacy_tickets_have_no_parsed_fields() {
        let proto = ticket_to_proto(TicketInfo {
            value: "sesame".to_string(),
            kind: TicketKind::Legacy,
            session_id: None,
            player_id: None,
            expires_at_unix: None,
        });

        assert_eq!(proto.kind, v1::TicketKind::Legacy as i32);
        assert_eq!(proto.value, "sesame");
        assert!(proto.session_id.is_none());
        assert!(proto.player_id.is_none());
        assert!(proto.expires_at_unix.is_none());
    }

    #[test]
    fn create_game_request_fields_map_to_game_options() {
        let options = game_options_from_proto(v1::CreateGameRequest {
            name: Some("2v2".to_string()),
            world: Some(3),
            timer: Some(30),
            endturn: Some(15),
            play_mode: Some(1),
            start_gold: Some(200),
            victory_type: Some(0),
            victory_limit: Some(100),
            min_players: Some(4),
            password: Some("sesame".to_string()),
        });

        assert_eq!(
            options,
            GameOptions {
                name: Some("2v2".to_string()),
                world: Some(3),
                timer: Some(30),
                endturn: Some(15),
                play_mode: Some(1),
                start_gold: Some(200),
                victory_type: Some(0),
                victory_limit: Some(100),
                min_players: Some(4),
            }
        );
    }

    #[test]
    fn service_error_keeps_the_legacy_message_as_status() {
        let error = crate::error::ServiceError::from_ws("game not found");
        let status = Status::from(error);

        assert_eq!(status.code(), tonic::Code::NotFound);
        assert_eq!(status.message(), "game not found");
    }
}
