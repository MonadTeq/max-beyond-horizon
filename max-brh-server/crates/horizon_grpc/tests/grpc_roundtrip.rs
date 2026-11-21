//! Integration test of the `horizon.v1.Horizon` gRPC server.
//!
//! Every test boots a real tonic server on `127.0.0.1:0` (so tests can run in
//! parallel), talks to it through the generated `horizon.v1` client, and uses
//! [`MockSessions`] as the session service: the whole adapter — protobuf
//! conversion, lobby registry, password checks, signed tickets, admin
//! authorization, error mapping — runs for real, only the `max-port` process
//! spawning is faked.
//!
//! This is the gRPC counterpart of a `horizon-smoke.py` run: `Hello`,
//! `ListGames` (empty), `CreateGame`, `JoinGame` with a ticket, the five admin
//! RPCs, `Ping`, and the documented error statuses.

use std::sync::Arc;
use std::time::Duration;

use horizon_grpc::admin::admin_token_for;
use horizon_grpc::proto::v1 as pb;
use horizon_grpc::testing::MockSessions;
use horizon_grpc::{serve_with_listener, HorizonBackend, MaxBackend};
use max_auth::Auth;
use max_session::SessionStatus;
use tokio::net::TcpListener;
use tonic::Code;
use tonic::transport::Channel;
use uuid::Uuid;

const SESSION_SECRET: &str = "integration-session-secret";
const ADMIN_SECRET: &str = "integration-admin-secret";

type Client = pb::horizon_client::HorizonClient<Channel>;

/// A backend with a fake session service, the two secrets, and a small port range.
fn backend(sessions: Arc<MockSessions>, session_secret: Option<&str>) -> Arc<dyn HorizonBackend> {
    Arc::new(MaxBackend::new(
        Some(sessions as Arc<dyn horizon_grpc::SessionControl>),
        session_secret.map(str::to_string),
        Some(ADMIN_SECRET.to_string()),
        32000..=32010,
    ))
}

/// Boots the gRPC server on an ephemeral port and returns the client plus the
/// handles needed to shut it down.
async fn start(
    backend: Arc<dyn HorizonBackend>,
) -> (Client, tokio::task::JoinHandle<()>, std::net::SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind an ephemeral gRPC port");
    let address = listener.local_addr().expect("local address");
    let handle = tokio::spawn(async move {
        // The test aborts the task, so the shutdown future never resolves.
        let _ = serve_with_listener(backend, listener, std::future::pending::<()>()).await;
    });

    let endpoint = format!("http://{address}");
    let channel = Channel::from_shared(endpoint)
        .expect("valid endpoint")
        .connect_timeout(Duration::from_secs(5))
        .connect()
        .await
        .expect("connect to the gRPC server");

    (pb::horizon_client::HorizonClient::new(channel), handle, address)
}

fn hello(version: u32) -> pb::HelloRequest {
    pb::HelloRequest {
        protocol_version: version,
        client: "max_net/0.1".to_string(),
        dictionary_id: None,
    }
}

fn create_request(name: &str, min_players: u32, password: Option<&str>) -> pb::CreateGameRequest {
    pb::CreateGameRequest {
        name: Some(name.to_string()),
        world: Some(3),
        min_players: Some(min_players),
        password: password.map(str::to_string),
        ..pb::CreateGameRequest::default()
    }
}

fn admin_token() -> String {
    admin_token_for(ADMIN_SECRET)
}

// ============================================================================
// hello / ping
// ============================================================================

#[tokio::test]
async fn hello_acknowledges_the_protocol_and_ping_answers_pong() {
    let sessions = MockSessions::new();
    let (mut client, handle, _) = start(backend(sessions, None)).await;

    let ack = client
        .hello(hello(1))
        .await
        .expect("hello succeeds")
        .into_inner();
    assert_eq!(ack.protocol_version, 1);
    assert_eq!(ack.format, "protobuf");
    assert_eq!(ack.compression, "none");
    assert!(ack.dictionary_id.is_none());

    let pong = client
        .ping(pb::PingRequest {
            payload: Some(b"hi".to_vec()),
        })
        .await
        .expect("ping succeeds")
        .into_inner();
    assert_eq!(pong.message, "pong");
    assert!(pong.timestamp > 0, "timestamp must be wall-clock millis");

    // WS close 4000 parity: unsupported version -> FAILED_PRECONDITION.
    let status = client.hello(hello(2)).await.unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert_eq!(status.message(), "protocol version not supported");

    // uint32 -> u16 validation.
    let status = client.hello(hello(70_000)).await.unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument);
    assert!(status.message().contains("does not fit u16"));

    handle.abort();
}

// ============================================================================
// lobby
// ============================================================================

#[tokio::test]
async fn lobby_lists_creates_and_joins_with_a_signed_ticket() {
    let sessions = MockSessions::new();
    let mock = Arc::clone(&sessions);
    let (mut client, handle, _) = start(backend(sessions, Some(SESSION_SECRET))).await;

    // Empty lobby snapshot.
    let games = client
        .list_games(pb::ListGamesRequest {})
        .await
        .expect("list games")
        .into_inner()
        .games;
    assert!(games.is_empty(), "a fresh backend has no games");

    // Create a protected game (the password doubles as the legacy ticket).
    let created = client
        .create_game(create_request("Friday 2v2", 4, Some("sesame")))
        .await
        .expect("create game")
        .into_inner();
    let game = created.game.expect("CreateGameResponse.game");
    let game_id = Uuid::parse_str(&game.id).expect("game id is a UUID");
    assert_eq!(game.name, "Friday 2v2");
    assert_eq!(game.world, 3);
    assert_eq!(game.min_players, 4);
    assert_eq!(game.players, 1);
    assert_eq!(game.status, pb::GameStatus::Starting as i32);
    assert!(game.password_protected);

    let creator_ticket = created.ticket.expect("CreateGameResponse.ticket");
    assert_eq!(creator_ticket.value, "sesame");
    assert_eq!(creator_ticket.kind, pb::TicketKind::Legacy as i32);

    // Joining a game that is not Running yet: FAILED_PRECONDITION with the WS text.
    let status = client
        .join_game(pb::JoinGameRequest {
            game_id: game.id.clone(),
            password: Some("sesame".to_string()),
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::FailedPrecondition);
    assert_eq!(status.message(), "game is not running (status: starting)");

    // The fake manager flips the session to Running (a real one does once spawned).
    assert!(mock.set_status(game_id, SessionStatus::Running));

    // Wrong password: PERMISSION_DENIED.
    let status = client
        .join_game(pb::JoinGameRequest {
            game_id: game.id.clone(),
            password: Some("wrong".to_string()),
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::PermissionDenied);
    assert_eq!(status.message(), "invalid password");

    // Correct password: host/port + a signed per-player ticket.
    let joined = client
        .join_game(pb::JoinGameRequest {
            game_id: game.id.clone(),
            password: Some("sesame".to_string()),
        })
        .await
        .expect("join game")
        .into_inner();
    assert_eq!(joined.game_id, game.id);
    assert_eq!(joined.host, "127.0.0.1");
    assert_eq!(joined.port, game.port);

    let ticket = joined.ticket.expect("JoinGameResponse.ticket");
    assert_eq!(ticket.kind, pb::TicketKind::Signed as i32);
    assert!(ticket.value.starts_with("v1."));
    let game_id_string = game_id.to_string();
    assert_eq!(ticket.session_id.as_deref(), Some(game_id_string.as_str()));
    assert!(ticket.player_id.is_some());
    assert!(ticket.expires_at_unix.is_some());

    // The ticket is exactly what max_auth verifies against the session.
    let verified = Auth::new(SESSION_SECRET)
        .verify(&ticket.value, game_id)
        .expect("the signed ticket verifies");
    assert_eq!(verified.player_id.to_string(), ticket.player_id.unwrap());
    assert_eq!(verified.expires_at_unix, ticket.expires_at_unix.unwrap());

    // The lobby snapshot now reports the game with two players.
    let games = client
        .list_games(pb::ListGamesRequest {})
        .await
        .expect("list games")
        .into_inner()
        .games;
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].id, game.id);
    assert_eq!(games[0].players, 2);
    assert_eq!(games[0].status, pb::GameStatus::Running as i32);

    handle.abort();
}

#[tokio::test]
async fn lobby_errors_use_the_documented_statuses() {
    let sessions = MockSessions::new();
    let (mut client, handle, _) = start(backend(sessions, None)).await;

    // Unknown game id -> NOT_FOUND.
    let status = client
        .join_game(pb::JoinGameRequest {
            game_id: Uuid::new_v4().to_string(),
            password: None,
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::NotFound);
    assert_eq!(status.message(), "game not found");

    // Not a UUID -> also NOT_FOUND (indistinguishable in the WS protocol).
    let status = client
        .join_game(pb::JoinGameRequest {
            game_id: "not-a-uuid".to_string(),
            password: None,
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::NotFound);
    assert_eq!(status.message(), "game not found");

    // Fewer than two players -> INVALID_ARGUMENT with the WS text.
    let status = client
        .create_game(create_request("solo", 1, None))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument);
    assert_eq!(
        status.message(),
        "a network game needs at least 2 players"
    );

    // A process/manager failure surfaces as a status with its own text.
    let sessions = MockSessions::new();
    sessions.set_start_error(Some(
        "no free UDP port left in the configured range".to_string(),
    ));
    let (mut client, handle2, _) = start(backend(sessions, None)).await;
    let status = client
        .create_game(create_request("full", 2, None))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::ResourceExhausted);
    assert_eq!(
        status.message(),
        "no free UDP port left in the configured range"
    );

    handle.abort();
    handle2.abort();
}

// ============================================================================
// admin
// ============================================================================

#[tokio::test]
async fn admin_requires_the_token_and_drives_the_session_lifecycle() {
    let sessions = MockSessions::new();
    let mock = Arc::clone(&sessions);
    let (mut client, handle, _) = start(backend(sessions, None)).await;

    // Fail-closed authorization: the WS `admin_error` becomes a status.
    let status = client
        .admin_list_sessions(pb::AdminListSessionsRequest { admin_token: None })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::Unauthenticated);
    assert_eq!(status.message(), "missing admin_token");

    let status = client
        .admin_list_sessions(pb::AdminListSessionsRequest {
            admin_token: Some("not-the-token".to_string()),
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::Unauthenticated);
    assert_eq!(status.message(), "invalid admin_token");

    // Authorized snapshot is empty, then contains the game created via the lobby.
    let listed = client
        .admin_list_sessions(pb::AdminListSessionsRequest {
            admin_token: Some(admin_token()),
        })
        .await
        .expect("authorized list")
        .into_inner()
        .sessions;
    assert!(listed.is_empty());

    let created = client
        .create_game(create_request("admin target", 2, None))
        .await
        .expect("create game")
        .into_inner()
        .game
        .expect("created game");
    let game_id = Uuid::parse_str(&created.id).expect("uuid");

    let listed = client
        .admin_list_sessions(pb::AdminListSessionsRequest {
            admin_token: Some(admin_token()),
        })
        .await
        .expect("authorized list")
        .into_inner()
        .sessions;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].game_id, created.id);
    assert_eq!(listed[0].state, pb::GameStatus::Starting as i32);
    assert!(listed[0].dir.contains(&created.id));

    // session_status: known vs unknown ids.
    let status = client
        .admin_session_status(pb::AdminSessionStatusRequest {
            game_id: created.id.clone(),
            admin_token: Some(admin_token()),
        })
        .await
        .expect("status")
        .into_inner();
    assert_eq!(status.game_id, created.id);
    assert_eq!(
        status.status.expect("status object").state,
        pb::GameStatus::Starting as i32
    );

    let unknown = client
        .admin_session_status(pb::AdminSessionStatusRequest {
            game_id: Uuid::new_v4().to_string(),
            admin_token: Some(admin_token()),
        })
        .await
        .expect("unknown status")
        .into_inner();
    assert_eq!(
        unknown.status.expect("status object").state,
        pb::GameStatus::Unknown as i32
    );

    // Malformed id -> INVALID_ARGUMENT (the WS `admin_error` text).
    let status = client
        .admin_session_status(pb::AdminSessionStatusRequest {
            game_id: "abc".to_string(),
            admin_token: Some(admin_token()),
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument);
    assert_eq!(status.message(), "invalid game_id \"abc\": expected a UUID");

    // start_session: the manager allocates the port itself.
    let started = client
        .admin_session_start(pb::AdminSessionStartRequest {
            admin_token: Some(admin_token()),
            port: None,
            name: Some("admin started".to_string()),
            ..pb::AdminSessionStartRequest::default()
        })
        .await
        .expect("start session")
        .into_inner();
    assert!(started.ok);
    let started_id = Uuid::parse_str(started.game_id.as_deref().expect("game_id")).expect("uuid");
    assert!(started.port.is_some());
    assert!(mock.contains(started_id));

    // restart_session: the old id goes away, a fresh one appears.
    let restarted = client
        .admin_session_restart(pb::AdminSessionRestartRequest {
            game_id: game_id.to_string(),
            admin_token: Some(admin_token()),
        })
        .await
        .expect("restart session")
        .into_inner();
    assert!(restarted.ok);
    assert_eq!(restarted.game_id, game_id.to_string());
    let new_id = Uuid::parse_str(restarted.new_game_id.as_deref().expect("new_game_id"))
        .expect("fresh uuid");
    assert_ne!(new_id, game_id);
    assert!(restarted.port.is_some());
    assert!(!mock.contains(game_id), "the old session is gone");
    assert!(mock.contains(new_id), "the fresh session is tracked");

    // stop_session: ok, then an inline NOT_FOUND error for the second call.
    let stopped = client
        .admin_session_stop(pb::AdminSessionStopRequest {
            game_id: new_id.to_string(),
            admin_token: Some(admin_token()),
        })
        .await
        .expect("stop session")
        .into_inner();
    assert!(stopped.ok);
    assert!(stopped.error.is_none());
    assert!(!mock.contains(new_id));

    let stopped_again = client
        .admin_session_stop(pb::AdminSessionStopRequest {
            game_id: new_id.to_string(),
            admin_token: Some(admin_token()),
        })
        .await
        .expect("stop unknown session returns an inline failure")
        .into_inner();
    assert!(!stopped_again.ok);
    let inline = stopped_again.error.expect("inline error");
    assert_eq!(inline.code, Code::NotFound as i32);
    assert_eq!(inline.legacy_close_code, 0);
    assert!(inline.message.contains("does not exist"));

    handle.abort();
}

#[tokio::test]
async fn admin_start_validates_the_requested_port_before_starting() {
    let sessions = MockSessions::new();
    let mock = Arc::clone(&sessions);
    let (mut client, handle, _) = start(backend(sessions, None)).await;

    // Outside MAX_PORT_RANGE -> INVALID_ARGUMENT status.
    let status = client
        .admin_session_start(pb::AdminSessionStartRequest {
            admin_token: Some(admin_token()),
            port: Some(4000),
            ..pb::AdminSessionStartRequest::default()
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::InvalidArgument);
    assert!(
        status.message().contains("outside the configured MAX_PORT_RANGE"),
        "message: {}",
        status.message()
    );

    // A running session on 32000 makes 32000 collide -> inline ALREADY_EXISTS.
    let existing = client
        .create_game(create_request("existing", 2, None))
        .await
        .expect("create game")
        .into_inner()
        .game
        .expect("created game");
    let existing_id = Uuid::parse_str(&existing.id).expect("uuid");
    assert!(mock.set_status(existing_id, SessionStatus::Running));

    let collision = client
        .admin_session_start(pb::AdminSessionStartRequest {
            admin_token: Some(admin_token()),
            port: Some(u32::from(existing.port)),
            ..pb::AdminSessionStartRequest::default()
        })
        .await
        .expect("collision is an inline failure, not a status")
        .into_inner();
    assert!(!collision.ok);
    let inline = collision.error.expect("inline error");
    assert_eq!(inline.code, Code::AlreadyExists as i32);
    assert!(inline.message.contains("is already used by session"));

    // A free port inside the range starts normally.
    let started = client
        .admin_session_start(pb::AdminSessionStartRequest {
            admin_token: Some(admin_token()),
            port: Some(32005),
            name: Some("port pre-check".to_string()),
            ..pb::AdminSessionStartRequest::default()
        })
        .await
        .expect("start session")
        .into_inner();
    assert!(started.ok);
    assert!(started.game_id.is_some());

    handle.abort();
}

#[tokio::test]
async fn unconfigured_lobby_and_admin_answer_unavailable() {
    let unconfigured = Arc::new(MaxBackend::new(
        None,
        None,
        Some(ADMIN_SECRET.to_string()),
        32000..=32010,
    ));
    let (mut client, handle, _) = start(unconfigured).await;

    let status = client
        .list_games(pb::ListGamesRequest {})
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);
    assert_eq!(status.message(), "max_lobby is not configured");

    let status = client
        .create_game(create_request("x", 2, None))
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);
    assert_eq!(status.message(), "max_lobby is not configured");

    let status = client
        .admin_list_sessions(pb::AdminListSessionsRequest {
            admin_token: Some(admin_token()),
        })
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::Unavailable);
    assert_eq!(status.message(), "max_admin is not configured");

    handle.abort();
}

#[tokio::test]
async fn stop_then_shutdown_releases_the_backend_sessions() {
    let sessions = MockSessions::new();
    let mock = Arc::clone(&sessions);
    let (mut client, handle, _) = start(backend(sessions, None)).await;

    client
        .create_game(create_request("session to stop", 2, None))
        .await
        .expect("create game");
    assert_eq!(mock.len(), 1);

    // `serve_with_listener` calls `shutdown_all` when its shutdown signal fires;
    // aborting the task drops the server, which is the test's teardown path.
    handle.abort();
    let _ = handle.await;
}
