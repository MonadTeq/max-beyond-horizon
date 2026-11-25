use super::*;
use crate::enet;
use crate::handshake::{HandshakeError, RejectReason};
use crate::session::{Session, SessionConfig, SessionError, SessionEvent, SessionHandle};
use max_types::input::{Command, Packet18Chat};
use max_types::team::TeamId;
use std::net::UdpSocket;
use tokio::time::Instant;

/// Blocking service timeout of the raw (non-tokio) test clients.
const RAW_SERVICE_MS: u32 = 10;

fn data_hash() -> DataHash {
    DataHash::from_bytes(b"max", b"patches")
}

fn wrong_data_hash() -> DataHash {
    DataHash::from_bytes(b"other", b"patches")
}

/// An ephemeral UDP port announced by the OS (same probe as `session.rs`'s
/// integration test).
fn free_udp_port() -> u16 {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("probe socket");
    socket.local_addr().expect("probe addr").port()
}

fn start_host(port: u16, heartbeat: Duration) -> HostHandle {
    let config = HostConfig {
        bind_ip: Ipv4Addr::LOCALHOST,
        port,
        game_version: DEFAULT_GAME_VERSION,
        data_hash: data_hash(),
        max_peers: DEFAULT_MAX_PEERS,
        heartbeat,
        host_timeout: DEFAULT_HOST_TIMEOUT,
    };
    HostHandle::bind(config).expect("bind host")
}

fn chat(sender: TeamId, message: &str) -> Packet {
    Packet::Command(Command::Chat(Packet18Chat {
        sender,
        message: message.to_string(),
    }))
}

async fn connect_guest(port: u16, hash: DataHash) -> SessionHandle {
    let config = SessionConfig::new("127.0.0.1", port, "", hash);
    Session::connect(config)
        .await
        .expect("guest TP 00 exchange")
        .start()
}

async fn wait_host_event(host: &mut HostHandle, timeout: Duration) -> HostEvent {
    host.next_event_timeout(timeout)
        .await
        .expect("host event timeout")
        .expect("host task ended")
}

async fn wait_verified(host: &mut HostHandle, timeout: Duration) -> PeerId {
    loop {
        let event = wait_host_event(host, timeout).await;
        if let HostEventKind::Verified = event.kind {
            return event.peer;
        }
    }
}

async fn wait_host_packet(host: &mut HostHandle, timeout: Duration) -> (PeerId, Packet) {
    loop {
        let event = wait_host_event(host, timeout).await;
        if let HostEventKind::Packet(packet) = event.kind {
            return (event.peer, packet);
        }
    }
}

/// Asserts that no `Packet` event arrives within `timeout`.
async fn assert_no_host_packet(host: &mut HostHandle, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return;
        }
        match host.next_event_timeout(remaining).await {
            Ok(Some(event)) => {
                if let HostEventKind::Packet(packet) = event.kind {
                    panic!("unexpected host packet from {}: {packet:?}", event.peer);
                }
            }
            Ok(None) => panic!("host task ended"),
            Err(_) => return,
        }
    }
}

async fn wait_guest_packet(client: &mut SessionHandle, timeout: Duration) -> Packet {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
                "guest packet did not arrive within {timeout:?}"
        );
        match client
            .next_event_timeout(remaining)
            .await
            .expect("guest event timeout")
        {
            Some(SessionEvent::Packet(packet)) => return packet,
            Some(_) => continue,
            None => panic!("guest task ended"),
        }
    }
}

async fn assert_no_guest_packet(client: &mut SessionHandle, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return;
        }
        match client.next_event_timeout(remaining).await {
            Ok(Some(SessionEvent::Packet(packet))) => {
                panic!("unexpected guest packet: {packet:?}")
            }
            Ok(Some(_)) => continue,
            Ok(None) => panic!("guest task ended"),
            Err(_) => return,
        }
    }
}

/// Raw ENet client (no `Session`), used where the test must control the
/// transport handshake byte by byte.
fn raw_client(port: u16) -> Host {
    let mut host = Host::client(CHANNEL_COUNT).expect("raw ENet client");
    let address = enet::address(Ipv4Addr::LOCALHOST, port).expect("server address");
    host.connect(address, CHANNEL_COUNT).expect("raw connect");
    host.flush();
    host
}

fn wait_raw_connect(host: &mut Host, timeout: Duration) -> Peer {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let Some(event) = host.service(RAW_SERVICE_MS).expect("raw service") else {
            continue;
        };
        match event.kind {
            EventKind::Connect => return event.peer.expect("connect event has a peer"),
            EventKind::Disconnect => panic!("raw client disconnected during connect"),
            _ => {}
        }
    }
    panic!("ENet connect did not complete within {timeout:?}");
}

fn wait_tp_hello(host: &mut Host, timeout: Duration) -> Handshake {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let Some(event) = host.service(RAW_SERVICE_MS).expect("raw service") else {
            continue;
        };
        if event.kind == EventKind::Receive && event.channel == CHANNEL_TP {
            let payload = event.payload.expect("receive payload");
            if let Ok(TransportPacket::Hello(hello)) = TransportPacket::decode(&payload) {
                return hello;
            }
        }
        if event.kind == EventKind::Disconnect {
            panic!("raw client disconnected before the host TP 00");
        }
    }
    panic!("host TP 00 did not arrive within {timeout:?}");
}

fn wait_tp_reject(host: &mut Host, timeout: Duration) -> Reject {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let Some(event) = host.service(RAW_SERVICE_MS).expect("raw service") else {
            continue;
        };
        if event.kind == EventKind::Receive && event.channel == CHANNEL_TP {
            let payload = event.payload.expect("receive payload");
            if let Ok(TransportPacket::Reject(reject)) = TransportPacket::decode(&payload) {
                return reject;
            }
        }
        if event.kind == EventKind::Disconnect {
            panic!("raw client disconnected before TP 03");
        }
    }
    panic!("TP 03 did not arrive within {timeout:?}");
}

fn collect_appl_frames(host: &mut Host, count: usize, timeout: Duration) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let deadline = std::time::Instant::now() + timeout;
    while frames.len() < count && std::time::Instant::now() < deadline {
        let Some(event) = host.service(RAW_SERVICE_MS).expect("raw service") else {
            continue;
        };
        if event.kind == EventKind::Receive && event.channel == CHANNEL_APPL {
            frames.push(event.payload.expect("receive payload"));
        }
    }
    assert_eq!(frames.len(), count, "expected {count} relayed APPL frames");
    frames
}

/// (a) The host accepts N peers and both `TP 00` directions are exercised:
/// the guest validates the host's copy, and a guest with a mismatched data
/// hash is answered with `TP 03` (reason 3) and removed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn host_accepts_three_guests_and_rejects_a_mismatched_tp00() {
    let port = free_udp_port();
    let mut host = start_host(port, Duration::ZERO);

    let mut guests = Vec::new();
    let mut ids = Vec::new();
    for _ in 0..3 {
        guests.push(connect_guest(port, data_hash()).await);
        ids.push(wait_verified(&mut host, Duration::from_secs(5)).await);
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 3, "each accepted peer gets its own id");

    // Server -> client: the raw client reads the host's TP 00 (correct
    // version/data), then presents a wrong data hash.
    let mut raw = raw_client(port);
    let peer = wait_raw_connect(&mut raw, Duration::from_secs(5));
    let hello = wait_tp_hello(&mut raw, Duration::from_secs(5));
    assert_eq!(hello.game_version, DEFAULT_GAME_VERSION);
    assert_eq!(hello.enet_version, crate::handshake::ENET_VERSION);
    assert_eq!(hello.protocol_version, crate::handshake::PROTOCOL_VERSION);
    assert_eq!(hello.data_hash, data_hash().to_hex());
    assert_eq!(hello.ticket, "", "the host advertises an empty ticket");

    let wrong = Handshake::new(DEFAULT_GAME_VERSION, "", wrong_data_hash());
    raw.send(peer, CHANNEL_TP, &wrong.encode().expect("wrong TP 00"))
        .expect("send wrong TP 00");
    raw.flush();
    let reject = wait_tp_reject(&mut raw, Duration::from_secs(5));
    assert_eq!(reject.reason, RejectReason::GameData);
    assert_eq!(reject.reason_text, "game data");

    // The host dropped the mismatched peer: exactly one Connected
    // (before any of its frames) followed by one Disconnected, never
    // Verified.
    let mut rejected_id = None;
    loop {
        let event = wait_host_event(&mut host, Duration::from_secs(5)).await;
        match event.kind {
            HostEventKind::Connected => {
                assert!(rejected_id.is_none(), "second Connected for one peer");
                rejected_id = Some(event.peer);
            }
            HostEventKind::Disconnected => {
                assert_eq!(
                    Some(event.peer),
                    rejected_id,
                        "Disconnected before Connected"
                );
                break;
            }
            HostEventKind::Verified => panic!("a mismatched TP 00 must not verify"),
            HostEventKind::Packet(_) => {}
        }
    }

    // Client -> server: the unchanged `Session` guest rejects the host's
    // TP 00 on its own, sends TP 03 and surfaces the reason; the host must
    // drop that peer too (exactly one more Connected/Disconnected pair).
    let mismatched =
        Session::connect(SessionConfig::new("127.0.0.1", port, "", wrong_data_hash())).await;
    match mismatched {
        Err(SessionError::Handshake(HandshakeError::Rejected { reason, .. })) => {
            assert_eq!(reason, RejectReason::GameData)
        }
        other => panic!("expected a rejected handshake, got {other:?}"),
    }
    let mut rejected_id = None;
    loop {
        let event = wait_host_event(&mut host, Duration::from_secs(5)).await;
        match event.kind {
            HostEventKind::Connected => rejected_id = Some(event.peer),
            HostEventKind::Disconnected => {
                assert_eq!(Some(event.peer), rejected_id);
                break;
            }
            HostEventKind::Verified => panic!("a mismatched TP 00 must not verify"),
            HostEventKind::Packet(_) => {}
        }
    }

    for guest in guests {
        guest.shutdown().await;
    }
    host.shutdown().await;
}

/// (b) A verified guest's frame reaches every other verified guest and
/// never the source ("multicast except source").
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn relay_reaches_every_verified_guest_except_the_source() {
    let port = free_udp_port();
    let mut host = start_host(port, Duration::ZERO);

    let mut guests = Vec::new();
    let mut ids = Vec::new();
    for _ in 0..3 {
        guests.push(connect_guest(port, data_hash()).await);
        ids.push(wait_verified(&mut host, Duration::from_secs(5)).await);
    }

    let frame = chat(TeamId::RED, "relay");
    guests[0].send(frame.clone()).await.expect("send chat");

    let (source, packet) = wait_host_packet(&mut host, Duration::from_secs(5)).await;
    assert_eq!(source, ids[0], "host sees the producer id");
    assert_eq!(packet, frame);

    for guest in guests.iter_mut().skip(1) {
        assert_eq!(
            wait_guest_packet(guest, Duration::from_secs(5)).await,
            frame,
                "relayed copy is unchanged"
        );
    }
    assert_no_guest_packet(&mut guests[0], Duration::from_millis(300)).await;

    for guest in guests {
        guest.shutdown().await;
    }
    host.shutdown().await;
}

/// (c) Relay is FIFO and byte-exact: a raw receiver sees the sender's
/// encoded frames in order, with no re-encoding.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn relay_preserves_fifo_order_and_exact_bytes() {
    let port = free_udp_port();
    let mut host = start_host(port, Duration::ZERO);

    let sender = connect_guest(port, data_hash()).await;
    let _sender_id = wait_verified(&mut host, Duration::from_secs(5)).await;

    // Raw receiver: no Session decoding, so the test compares wire bytes.
    let mut receiver = raw_client(port);
    let receiver_peer = wait_raw_connect(&mut receiver, Duration::from_secs(5));
    receiver
        .send(
            receiver_peer,
            CHANNEL_TP,
            &Handshake::new(DEFAULT_GAME_VERSION, "", data_hash())
                .encode()
                .expect("TP 00"),
        )
        .expect("send TP 00");
    receiver.flush();
    let _receiver_id = wait_verified(&mut host, Duration::from_secs(5)).await;

    let mut expected = Vec::new();
    for counter in 0..5 {
        let frame = chat(TeamId::GREEN, &format!("fifo-{counter}"));
        let bytes = frame.encode().expect("encode chat");
        sender.send(frame).await.expect("send chat");
        expected.push(bytes);
    }

    let received = collect_appl_frames(&mut receiver, expected.len(), Duration::from_secs(5));
    assert_eq!(received, expected, "relay keeps bytes and order");

    sender.shutdown().await;
    host.shutdown().await;
}

/// (d) APPL frames from a peer that has not passed `TP 00` are dropped;
/// once verified, the same peer's frames are accepted and relayed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn appl_from_unverified_peer_is_dropped() {
    let port = free_udp_port();
    let mut host = start_host(port, Duration::ZERO);

    let mut witness = connect_guest(port, data_hash()).await;
    let _witness_id = wait_verified(&mut host, Duration::from_secs(5)).await;

    let mut unverified = raw_client(port);
    let unverified_peer = wait_raw_connect(&mut unverified, Duration::from_secs(5));

    // APPL before TP 00: the host must neither emit nor relay it.
    let early = chat(TeamId::RED, "before-tp00");
    unverified
        .send(
            unverified_peer,
            CHANNEL_APPL,
            &early.encode().expect("encode early chat"),
        )
        .expect("send early APPL");
    unverified.flush();

    // Wait for the host's Connected event of this peer first, then make
    // sure no Packet event follows within the window.
    let unverified_id = loop {
        let event = wait_host_event(&mut host, Duration::from_secs(5)).await;
        if let HostEventKind::Connected = event.kind {
            break event.peer;
        }
    };
    assert_no_host_packet(&mut host, Duration::from_millis(300)).await;
    assert_no_guest_packet(&mut witness, Duration::from_millis(300)).await;

    // Verify the same peer, then send the same frame again.
    unverified
        .send(
            unverified_peer,
            CHANNEL_TP,
            &Handshake::new(DEFAULT_GAME_VERSION, "", data_hash())
                .encode()
                .expect("TP 00"),
        )
        .expect("send TP 00");
    unverified.flush();
    assert_eq!(
        wait_verified(&mut host, Duration::from_secs(5)).await,
        unverified_id
    );

    let (source, packet) = {
        unverified
            .send(
                unverified_peer,
                CHANNEL_APPL,
                &early.encode().expect("encode chat"),
            )
            .expect("send verified APPL");
        unverified.flush();
        wait_host_packet(&mut host, Duration::from_secs(5)).await
    };
    assert_eq!(source, unverified_id);
    assert_eq!(packet, early);
    assert_eq!(
        wait_guest_packet(&mut witness, Duration::from_secs(5)).await,
        early,
            "a verified frame is relayed to the other guests"
    );

    witness.shutdown().await;
    host.shutdown().await;
}

/// `HostHandle::send` addresses one peer or broadcasts; unverified peers
/// are skipped.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn host_send_unicast_and_broadcast_reach_verified_guests() {
    let port = free_udp_port();
    let mut host = start_host(port, Duration::ZERO);

    let mut guest_a = connect_guest(port, data_hash()).await;
    let id_a = wait_verified(&mut host, Duration::from_secs(5)).await;
    let mut guest_b = connect_guest(port, data_hash()).await;
    let id_b = wait_verified(&mut host, Duration::from_secs(5)).await;
    assert_ne!(id_a, id_b);

    let unicast = chat(TeamId::RED, "only-b");
    host.send(id_b, unicast.clone()).await.expect("unicast");
    host.flush().await.expect("flush");
    assert_eq!(
        wait_guest_packet(&mut guest_b, Duration::from_secs(5)).await,
        unicast
    );
    assert_no_guest_packet(&mut guest_a, Duration::from_millis(300)).await;

    let broadcast = chat(TeamId::RED, "everyone");
    host.send(Recipient::Broadcast, broadcast.clone())
        .await
        .expect("broadcast");
    assert_eq!(
        wait_guest_packet(&mut guest_a, Duration::from_secs(5)).await,
        broadcast
    );
    assert_eq!(
        wait_guest_packet(&mut guest_b, Duration::from_secs(5)).await,
        broadcast
    );

    guest_a.shutdown().await;
    guest_b.shutdown().await;
    host.shutdown().await;
}

/// An incoming `TP 03` (the guest refuses our copy) drops the peer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn incoming_tp03_drops_the_peer() {
    let port = free_udp_port();
    let mut host = start_host(port, Duration::ZERO);

    let mut raw = raw_client(port);
    let peer = wait_raw_connect(&mut raw, Duration::from_secs(5));
    raw.send(
        peer,
        CHANNEL_TP,
        &Handshake::new(DEFAULT_GAME_VERSION, "", data_hash())
            .encode()
            .expect("TP 00"),
    )
    .expect("send TP 00");
    raw.flush();
    let id = wait_verified(&mut host, Duration::from_secs(5)).await;

    let reject = Reject::new(
        &Handshake::new(DEFAULT_GAME_VERSION, "", data_hash()),
        RejectReason::Version,
    );
    raw.send(peer, CHANNEL_TP, &reject.encode().expect("TP 03"))
        .expect("send TP 03");
    raw.flush();

    let event = wait_host_event(&mut host, Duration::from_secs(5)).await;
    assert_eq!(event.peer, id);
    assert!(
        matches!(event.kind, HostEventKind::Disconnected),
            "a TP 03 removes the peer: {:?}",
        event.kind
    );

    host.shutdown().await;
}

/// Packet 55 is a transport frame with its own wire type, not a `Packet`.
#[test]
fn heartbeat_frame_is_packet_55_and_round_trips() {
    assert_eq!(HOST_HEARTBEAT_PACKET_NUMBER, 55);
    assert_eq!(HOST_HEARTBEAT_TYPE, 0x87);
    assert_eq!(HOST_HEARTBEAT_TYPE, TRANSPORT_APPL_PACKET_ID + 55);

    let heartbeat = HostHeartbeat {
        host_epoch: 3,
        session_tick: 9,
        turn_counter: 7,
        phase: 2,
        reachable: 0b101,
        last_seen: [9, 8, 7, 6],
        policy: HostSessionPolicy::EliminateAfter { ticks: 12 },
        host_timeout_ms: 5_000,
        quorum: 2,
        eliminated: 0b0100,
        eliminated_tick: [0, 0, 5, 0],
    };
    let frame = heartbeat.encode();
    assert_eq!(frame.len(), 64, "type + 63 body bytes");
    assert_eq!(frame[0], 0x87);
    // Little-endian fixed positions of the scalar fields.
    assert_eq!(&frame[1..5], &3u32.to_le_bytes()); // host_epoch
    assert_eq!(&frame[5..9], &9u32.to_le_bytes()); // session_tick
    assert_eq!(&frame[9..13], &7i32.to_le_bytes()); // turn_counter
    assert_eq!(frame[13], 2); // phase
    assert_eq!(&frame[14..18], &0b101u32.to_le_bytes()); // reachable
    assert_eq!(&frame[18..22], &9u32.to_le_bytes()); // last_seen[0]
    assert_eq!(frame[34], 1); // policy tag: eliminate
    assert_eq!(&frame[35..39], &12u32.to_le_bytes()); // policy ticks
    assert_eq!(&frame[39..43], &5_000u32.to_le_bytes()); // host_timeout
    assert_eq!(frame[43], 2); // quorum
    assert_eq!(&frame[44..48], &0b0100u32.to_le_bytes()); // eliminated
    assert_eq!(&frame[48..52], &0u32.to_le_bytes()); // eliminated_tick[0]
    assert_eq!(&frame[56..60], &5u32.to_le_bytes()); // eliminated_tick[2]
    assert_eq!(HostHeartbeat::decode(&frame).expect("decode"), heartbeat);
    assert_eq!(
        HostHeartbeat::decode_payload(&frame[1..]).expect("payload decode"),
        heartbeat
    );

    let mut wrong_type = frame.clone();
    wrong_type[0] = 0x86;
    assert!(HostHeartbeat::decode(&wrong_type).is_err());

    assert!(
        HostHeartbeat::decode(&frame[..frame.len() - 1]).is_err(),
            "truncated body"
    );

    let mut trailing = frame.clone();
    trailing.push(0);
    assert!(HostHeartbeat::decode(&trailing).is_err(), "trailing bytes");
}

/// `host_timeout` is validated in the 5..=300 s range and is independent
/// of the heartbeat repeat period.
#[test]
fn host_timeout_is_validated_and_separate_from_the_heartbeat() {
    assert_eq!(DEFAULT_HOST_TIMEOUT, Duration::from_secs(90));
    assert_eq!(HOST_TIMEOUT_MIN, Duration::from_secs(5));
    assert_eq!(HOST_TIMEOUT_MAX, Duration::from_secs(300));

    for valid in [
        Duration::from_secs(5),
        Duration::from_secs(90),
        Duration::from_secs(300),
    ] {
        assert!(validate_host_timeout(valid).is_ok(), "{valid:?}");
    }
    for invalid in [
        Duration::ZERO,
        Duration::from_secs(4),
        Duration::from_secs(301),
    ] {
        assert!(
            matches!(validate_host_timeout(invalid), Err(HostError::Config(_))),
                "{invalid:?} must be rejected"
        );
    }

    // The range is enforced on `HostSession::bind`, not only by the CLI.
    let mut config = HostConfig::new(Ipv4Addr::LOCALHOST, 0, data_hash());
    config.host_timeout = Duration::from_secs(2);
    assert!(matches!(
        HostSession::bind(config),
        Err(HostError::Config(_))
    ));
}

/// Packet 56 round-trips a proposal with the full roster and a bare ack.
#[test]
fn takeover_frame_is_packet_56_and_round_trips() {
    assert_eq!(HOST_TAKEOVER_PACKET_NUMBER, 56);
    assert_eq!(HOST_TAKEOVER_TYPE, 0x88);
    assert_eq!(HOST_TAKEOVER_TYPE, TRANSPORT_APPL_PACKET_ID + 56);

    let proposal = Takeover {
        kind: TakeoverKind::Proposal,
        epoch: 2,
        turn_counter: 5,
        phase: 3,
        counters: TakeoverCounters {
            begin: 5,
            end: 5,
            sync: 5,
            crc: 4,
        },
        node: 11,
        roster: vec![
            TakeoverRosterEntry {
                node: 10,
                team_slot: 0,
                reachable: false,
            },
            TakeoverRosterEntry {
                node: 11,
                team_slot: 1,
                reachable: true,
            },
            TakeoverRosterEntry {
                node: 12,
                team_slot: 2,
                reachable: true,
            },
        ],
    };
    let frame = proposal.encode();
    assert_eq!(frame[0], HOST_TAKEOVER_TYPE);
    assert_eq!(Takeover::decode(&frame).expect("decode"), proposal);
    assert_eq!(
        Takeover::decode_payload(&proposal.encode_payload()).expect("payload"),
        proposal
    );

    let ack = Takeover {
        kind: TakeoverKind::Accept,
        roster: Vec::new(),
        ..proposal.clone()
    };
    assert_eq!(Takeover::decode(&ack.encode()).expect("ack"), ack);

    // Unknown kind, too large roster and trailing bytes are rejected.
    let mut bad = proposal.encode_payload();
    bad[0] = 9;
    assert!(Takeover::decode_payload(&bad).is_err());

    let mut oversized = proposal.encode_payload();
    oversized[16] = 5; // roster length > TAKEOVER_MAX_ROSTER
    assert!(Takeover::decode_payload(&oversized).is_err());

    let mut trailing = ack.encode();
    trailing.push(0);
    assert!(Takeover::decode(&trailing).is_err(), "trailing bytes");
}

/// The published heartbeat reaches guests immediately and is repeated on
/// the configured period; a late joiner receives the current one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn published_heartbeat_reaches_guests_and_repeats() {
    let port = free_udp_port();
    let mut host = start_host(port, Duration::from_millis(100));

    let mut guest = connect_guest(port, data_hash()).await;
    let guest_id = wait_verified(&mut host, Duration::from_secs(5)).await;

    let heartbeat = HostHeartbeat {
        host_epoch: 1,
        session_tick: 1,
        turn_counter: 0,
        phase: 1,
        reachable: 0b1,
        last_seen: [0; TAKEOVER_MAX_ROSTER],
        policy: HostSessionPolicy::Pause,
        host_timeout_ms: 90_000,
        quorum: 2,
        eliminated: 0,
        eliminated_tick: [0; TAKEOVER_MAX_ROSTER],
    };
    host.publish_heartbeat(heartbeat)
        .await
        .expect("publish heartbeat");

    // `Session` (unchanged, channel 1) decodes packet 55 as an unknown
    // APPL kind with the body preserved.
    let mut received = 0;
    let deadline = Instant::now() + Duration::from_secs(2);
    while received < 2 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
                "heartbeat was not repeated on the configured period"
        );
        match guest
            .next_event_timeout(remaining)
            .await
            .expect("guest event timeout")
        {
            Some(SessionEvent::Packet(Packet::Unknown { kind, payload })) => {
                assert_eq!(kind, HOST_HEARTBEAT_PACKET_NUMBER);
                let mut frame = vec![HOST_HEARTBEAT_TYPE];
                frame.extend_from_slice(&payload);
                assert_eq!(
                    HostHeartbeat::decode(&frame).expect("heartbeat body"),
                    heartbeat
                );
                received += 1;
            }
            Some(_) => {}
            None => panic!("guest task ended"),
        }
    }

    // A guest that verifies after the publish gets the current heartbeat.
    let mut late = connect_guest(port, data_hash()).await;
    let late_id = wait_verified(&mut host, Duration::from_secs(5)).await;
    assert_ne!(late_id, guest_id);
    match wait_guest_packet(&mut late, Duration::from_secs(5)).await {
        Packet::Unknown { kind, payload } => {
            assert_eq!(kind, HOST_HEARTBEAT_PACKET_NUMBER);
            let mut frame = vec![HOST_HEARTBEAT_TYPE];
            frame.extend_from_slice(&payload);
            assert_eq!(
                HostHeartbeat::decode(&frame).expect("late heartbeat"),
                heartbeat
            );
        }
        other => panic!("expected the heartbeat for the late joiner, got {other:?}"),
    }

    guest.shutdown().await;
    late.shutdown().await;
    host.shutdown().await;
}

#[test]
fn invalid_config_is_rejected_before_binding() {
    let mut config = HostConfig {
        port: 0,
        ..HostConfig::default()
    };
    config.max_peers = 0;
    assert!(matches!(
        HostSession::bind(config),
        Err(HostError::Config(_))
    ));

    let mut config = HostConfig {
        port: 0,
        ..HostConfig::default()
    };
    config.max_peers = PEER_COUNT + 1;
    assert!(matches!(
        HostHandle::bind(config),
        Err(HostError::Config(_))
    ));

    // A valid configuration binds an ephemeral port without a runtime.
    let session = HostSession::bind(HostConfig::new(Ipv4Addr::LOCALHOST, 0, data_hash()))
        .expect("bind ephemeral host");
    assert_eq!(session.local_handshake().game_version, DEFAULT_GAME_VERSION);
    assert_eq!(session.local_handshake().ticket, "");
    assert_eq!(session.config().port, 0);
}

/// Compile-time guarantee that the host handle is usable from a spawned
/// task (the session owns the ENet host, which is `Send` but not `Sync`).
#[test]
fn host_handle_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<HostHandle>();
    assert_send::<HostSession>();
    assert_send::<HostEvent>();
    assert_send::<HostHeartbeat>();
}
