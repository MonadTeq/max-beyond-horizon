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
