use super::*;
use max_types::input::{
    Packet00TurnBarrier, Packet01FrameBarrier, Packet18Chat, Packet45CrcBarrier,
    Packet52EndTurnBarrier,
};
use max_types::team::TeamId;

fn barrier_packet(counter: u8) -> Packet {
    Packet::Command(Command::EndTurnBarrier(Packet52EndTurnBarrier {
        team: TeamId::RED,
        counter,
    }))
}

#[test]
fn barrier_tracker_retransmits_and_times_out() {
    let start = Instant::now();
    let mut tracker = BarrierTracker::new(BarrierTiming {
        repeat: Duration::from_secs(3),
        timeout: Duration::from_secs(30),
    });
    tracker.track(barrier_packet(1), start);
    assert_eq!(tracker.len(), 1);

    assert!(tracker.poll(start + Duration::from_secs(1)).is_empty());
    let actions = tracker.poll(start + Duration::from_secs(3));
    assert!(matches!(
        actions.as_slice(),
        [BarrierAction::Retransmit { attempt: 1, .. }]
    ));
    let actions = tracker.poll(start + Duration::from_secs(6));
    assert!(matches!(
        actions.as_slice(),
        [BarrierAction::Retransmit { attempt: 2, .. }]
    ));
    assert_eq!(tracker.len(), 1);

    let actions = tracker.poll(start + Duration::from_secs(30));
    assert!(matches!(
        actions.as_slice(),
        [BarrierAction::TimedOut { .. }]
    ));
    assert!(tracker.is_empty());
}

#[test]
fn barrier_tracker_clear_stops_retransmission() {
    let start = Instant::now();
    let mut tracker = BarrierTracker::new(BarrierTiming::default());
    tracker.track(barrier_packet(1), start);
    tracker.clear();
    assert!(tracker.is_empty());
    assert!(tracker.poll(start + Duration::from_secs(60)).is_empty());
}

#[test]
fn tracked_barriers_are_only_00_45_52() {
    let turn = Packet::Command(Command::TurnBarrier(Packet00TurnBarrier {
        team: TeamId::RED,
        counter: 1,
    }));
    let crc = Packet::Command(Command::CrcBarrier(Packet45CrcBarrier {
        team: TeamId::RED,
        next_turn_index: 1,
        crc16: 0,
    }));
    assert!(is_tracked_barrier(&turn));
    assert!(is_tracked_barrier(&crc));
    assert!(is_tracked_barrier(&barrier_packet(1)));
    assert!(!is_tracked_barrier(&Packet::Command(Command::Chat(
        Packet18Chat {
            sender: TeamId::RED,
            message: "hi".into(),
        }
    ))));
}

/// Repeating a barrier through `SendUntracked` does not create a second tracker entry.
///
/// The old path (`SessionCommand::Send` on every retransmit) added one
/// entry per repeat — the number of concurrent repeats grew without bound.
#[test]
fn untracked_repeat_keeps_a_single_tracker_entry() {
    let now = Instant::now();
    let packet = barrier_packet(1);
    let mut tracker = BarrierTracker::new(BarrierTiming::default());

    assert!(records_barrier(true, &packet), "первый Send трекается");

    tracker.track(packet.clone(), now);
    assert_eq!(tracker.len(), 1);

    assert!(
        !records_barrier(false, &packet),
            "SendUntracked не должен трекать повтор"
    );

    // Exactly what `SessionCommand::SendUntracked` does: send only.
    assert_eq!(tracker.len(), 1, "повтор не добавил запись");

    // Whereas a repeat through Send would add one more — that was the double tracking.
    assert!(records_barrier(true, &packet));
    tracker.track(packet, now);
    assert_eq!(tracker.len(), 2);
}

#[test]
fn complex_summary_for_matches_team_complexes() {
    use max_core::team::TeamUnits;
    use max_types::enums::TeamClan;

    let mut state = max_core::universe::GameUniverse::default();
    state
        .team_mut(TeamId::RED)
        .expect("RED slot")
        .team_type = max_types::team::TeamKind::Player;
    *state.team_units_mut(TeamId::RED).expect("RED economy") =
        TeamUnits::with_clan(TeamClan::Random);
    let id = state
        .team_units_mut(TeamId::RED)
        .expect("RED economy")
        .create_complex();
    let complex = state
        .team_units_mut(TeamId::RED)
        .expect("RED economy")
        .get_complex_mut(id)
        .expect("complex created");
    complex.raw = 10;
    complex.fuel = 20;
    complex.gold = 30;
    complex.power = 40;
    complex.workers = 50;
    complex.buildings = 60;

    match complex_summary_for(state.snapshot(), TeamId::RED, 1) {
        Packet::Command(Command::ComplexSummary(summary)) => {
            assert_eq!(summary.team, TeamId::RED);
            assert_eq!(
                summary.turn, 1,
                    "turn — C++ GameManager_TurnCounter (1-based)"
            );
            assert_eq!(summary.complexes.len(), 1);
            assert_eq!(summary.complexes[0].complex_id, 1);
            assert_eq!(summary.complexes[0].values, [10, 20, 30, 40, 50, 60]);
        }
        other => panic!("expected packet 53, got {other:?}"),
    }
}

#[test]
fn crc_barrier_uses_max_core_checksum() {
    let state = max_core::universe::GameUniverse::default();
    let packet = crc_barrier_for(TeamId::RED, 3, state.snapshot());
    match packet {
        Packet::Command(Command::CrcBarrier(barrier)) => {
            assert_eq!(barrier.team, TeamId::RED);
            assert_eq!(barrier.next_turn_index, 3);
            assert_eq!(barrier.crc16, state.snapshot().state_checksum());
        }
        other => panic!("expected CRC barrier, got {other:?}"),
    }
}

#[test]
fn config_from_joined_copies_ticket_and_endpoint() {
    let joined = GameJoined {
        game_id: "g".into(),
        host: "127.0.0.1".into(),
        port: 31554,
        ticket: crate::horizon::Ticket {
            value: "sesame".into(),
            kind: crate::horizon::TicketKind::Legacy,
            session_id: None,
            player_id: None,
            expires_at_unix: None,
        },
    };
    let config = SessionConfig::from_joined(&joined, DataHash::from_bytes(b"a", b"b"));
    assert_eq!(config.host, "127.0.0.1");
    assert_eq!(config.port, 31554);
    assert_eq!(config.ticket, "sesame");
    assert_eq!(config.game_version, DEFAULT_GAME_VERSION);
    assert_eq!(parse_ipv4(&config.host), Some(Ipv4Addr::LOCALHOST));
    assert_eq!(ip_addr(Ipv4Addr::LOCALHOST).to_string(), "127.0.0.1");
}

/// The barrier command is taken from the payload's `team` field, not from the sender.
///
/// The relayed copy comes from the server (`Remote_GetSenderTeam` → -1 at the
/// C++ client), so the only source of the command is the packet bytes.
#[test]
fn barrier_team_comes_from_the_payload_team_field() {
    let frame = [
        0x50, 0x01, 0x00, 0x01, // 00: team=1, counter=1
    ];
    let packet = Packet::decode(&frame).expect("decode 00");
    assert_eq!(payload_team(&packet), Some(TeamId(1)));

    let frame = [0x84, 0x02, 0x00, 0x05]; // 52: team=2, counter=5
    let packet = Packet::decode(&frame).expect("decode 52");
    assert_eq!(payload_team(&packet), Some(TeamId(2)));

    let frame = [0x7D, 0x03, 0x00, 0x04, 0xCD, 0xAB]; // 45: team=3
    let packet = Packet::decode(&frame).expect("decode 45");
    assert_eq!(payload_team(&packet), Some(TeamId(3)));

    // Packets 06 and 53 also have a team field.
    let frame = [0x56, 0x01, 0x00];
    let packet = Packet::decode(&frame).expect("decode 06");
    assert_eq!(payload_team(&packet), Some(TeamId(1)));
}

/// Regression: ENet is polled even while the application sends continuously.
///
/// The old `select!` re-created `sleep(tick)` on every iteration, and
/// the stream of outgoing 01 (every few milliseconds) kept the timer from
/// firing: `pump` was not called, and relayed 00/52/45 sat in the socket until
/// the 30-second barrier timeout. The test brings up a real ENet server,
/// sends a relayed 00 and simultaneously floods the command channel.
#[test]
fn appl_frames_are_pumped_while_outgoing_commands_flood() {
    let port = {
        let socket = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("probe socket");
        socket.local_addr().expect("probe addr").port()
    };
    let data_hash = DataHash::from_bytes(b"max", b"patches");

    let server = std::thread::spawn(move || {
        let mut host =
            Host::server(Ipv4Addr::LOCALHOST, port, CHANNEL_COUNT).expect("test ENet server");
        let hello = Handshake::new(DEFAULT_GAME_VERSION, "", data_hash)
            .encode()
            .expect("server handshake");
        let relayed_00 = Packet::Command(Command::TurnBarrier(Packet00TurnBarrier {
            team: TeamId::GREEN,
            counter: 1,
        }))
        .encode()
        .expect("encode 00");

        let mut peer = None;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);

        while std::time::Instant::now() < deadline {
            let Some(event) = host.service(5).expect("server service") else {
                continue;
            };

            match event.kind {
                EventKind::Connect => peer = event.peer.or(peer),
                EventKind::Receive if event.channel == CHANNEL_TP => {
                    let Some(peer) = peer.or(event.peer) else {
                        continue;
                    };

                    host.send(peer, CHANNEL_TP, &hello).expect("send hello");
                    host.flush();

                    // Keep the channel busy: meanwhile the client floods
                    // the outgoing queue with 01 frames.
                    let until = std::time::Instant::now() + Duration::from_secs(3);
                    while std::time::Instant::now() < until {
                        host.send(peer, CHANNEL_APPL, &relayed_00)
                            .expect("send relayed 00");
                        host.flush();
                        let _ = host.service(10);
                    }

                    return;
                }
                _ => {}
            }
        }
    });

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    runtime.block_on(async move {
        let config = SessionConfig::new("127.0.0.1", port, "", data_hash);
        let mut handle = Session::connect(config).await.expect("connect").start();
        let frame_barrier = Packet::Command(Command::FrameBarrier(Packet01FrameBarrier {
            team: TeamId::RED,
            counter: 0,
        }));

        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);

        while tokio::time::Instant::now() < deadline {
            handle
                .send_untracked(frame_barrier.clone())
                .await
                .expect("send 01");

            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(1)) => {}
                event = handle.next_event() => {
                    if let Some(SessionEvent::Packet(Packet::Command(
                        Command::TurnBarrier(barrier),
                    ))) = event
                    {
                        assert_eq!(barrier.team, TeamId::GREEN, "team из payload");
                        assert_eq!(barrier.counter, 1);
                        return;
                    }
                }
            }
        }

        panic!("релейный 00 не дошёл: ENet не опрашивается при потоке исходящих команд");
    });

    server.join().expect("test ENet server thread");
}
