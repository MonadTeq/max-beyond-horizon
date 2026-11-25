use super::*;
use max_core::enums::{TeamClan, UnitType};
use max_core::input::{
    Packet00TurnBarrier, Packet01FrameBarrier, Packet06FinishedTurn, Packet13Seed,
    Packet18Chat, Packet45CrcBarrier, Packet52EndTurnBarrier, Packet53ComplexSummary,
};
use max_core::team::TeamUnits;
use max_core::universe::{NetworkCommand, SubmitOutcome};

/// Synthetic world over an arbitrary dispatcher: the local team is `Player`, the
/// peers are `Remote`, the rest `None` (`Remote_SetupPlayers` team types).
fn synthetic_universe_with<D: max_core::universe::Dispatcher>(
    local: TeamId,
    remotes: &[TeamId],
    dispatcher: D,
) -> max_core::universe::GameUniverse<D> {
    let mut state = max_core::universe::GameUniverse::with_dispatcher(dispatcher);
    for team in 0..PLAYER_TEAM_COUNT {
        state
            .team_mut(TeamId(team as u8))
            .expect("team slot")
            .team_type = TeamKind::None;
    }
    *state.team_units_table_mut() =
        std::array::from_fn(|_| TeamUnits::with_clan(TeamClan::Random));
    state.team_mut(local).expect("local slot").team_type = TeamKind::Player;
    for team in remotes {
        state.team_mut(*team).expect("remote slot").team_type = TeamKind::Remote;
    }
    state.set_player_team(local);
    state.set_play_mode(if remotes.is_empty() {
        PlayMode::TurnBased
    } else {
        PlayMode::TurnBased
    });
    state
}

/// A network client on the synthetic world — the stage C §4.2 surface at work
/// (`App<NetworkDispatcher>` owns the world and the barrier FSM).
fn network_client(
    local: TeamId,
    remotes: &[TeamId],
    play_mode: PlayMode,
) -> App<NetworkDispatcher> {
    let mut app = App::<NetworkDispatcher>::new_network(AppConfig::default(), local, play_mode);
    app.universe =
        synthetic_universe_with(local, remotes, NetworkDispatcher::new(local, play_mode));
    app
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
fn team_kinds_come_from_welcome_team_nodes() {
    let kinds = team_kinds_from_nodes(11, &[11, 0, 12, 0]);
    assert_eq!(kinds[0], TeamKind::Player, "своя нода → Player");
    assert_eq!(kinds[1], TeamKind::None, "слот 0 не занят");
    assert_eq!(kinds[2], TeamKind::Remote, "чужая нода → Remote");
    assert_eq!(kinds[3], TeamKind::None);
}

#[test]
fn team_kinds_ignore_nodes_at_or_below_four() {
    // Nodes 0..=4 are not clients (`Remote_SetupPlayers`: `> 4`).
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

#[test]
fn play_mode_comes_from_the_lobby_settings() {
    let mut settings = test_settings();
    settings.ini_play_mode = u8::from(true) as i32 * 0; // TurnBased = 0
    assert_eq!(play_mode_from_settings(&settings), PlayMode::TurnBased);

    settings.ini_play_mode = 1; // SimultaneousMoves = 1
    assert_eq!(
        play_mode_from_settings(&settings),
        PlayMode::SimultaneousMoves
    );
}

/// H3 gate (b): the histogram counts exactly the five barrier kinds.
#[test]
fn packet_histogram_counts_the_five_barrier_kinds() {
    let mut histogram = PacketHistogram::default();
    let sync = max_net::state_sync_for(
        max_core::universe::GameUniverse::default().snapshot(),
        TeamId::RED,
        1,
    )
    .pop()
    .expect("one empty-team chunk");

    histogram.note_all(&[
        Packet::Command(Command::TurnBarrier(Packet00TurnBarrier {
            team: TeamId::RED,
            counter: 1,
        })),
        Packet::Command(Command::ComplexSummary(Packet53ComplexSummary {
            team: TeamId::RED,
            turn: 1,
            complexes: Vec::new(),
        })),
        Packet::Command(Command::EndTurnBarrier(Packet52EndTurnBarrier {
            team: TeamId::RED,
            counter: 1,
        })),
        Packet::Command(Command::CrcBarrier(Packet45CrcBarrier {
            team: TeamId::RED,
            next_turn_index: 1,
            crc16: 0,
        })),
        Packet::StateSync(sync),
    ]);
    assert_eq!(histogram.to_line(), "00=1 45=1 52=1 53=1 54=1");

    // 01 and 06 are pacing/turn packets, not gate-(b) barrier kinds.
    histogram.note(&Packet::Command(Command::FrameBarrier(
        Packet01FrameBarrier {
            team: TeamId::RED,
            counter: 1,
        },
    )));
    histogram.note(&Packet::Command(Command::FinishedTurn(
        Packet06FinishedTurn { team: TeamId::RED },
    )));
    assert_eq!(histogram.to_line(), "00=1 45=1 52=1 53=1 54=1");
}

/// H3: the cross-client digest keeps only the values every client shares.
#[test]
fn shared_digest_drops_the_per_client_view_fields() {
    let digest = AppDigest::capture(max_core::universe::GameUniverse::default().snapshot(), 3);
    let shared = SharedDigest::capture(digest);

    assert_eq!(shared.ticks, 3);
    assert_eq!(shared.turn_counter, digest.turn_counter);
    assert_eq!(shared.game_state, digest.game_state);
    assert_eq!(shared.units, digest.units);
    assert_eq!(shared.state_checksum, digest.state_checksum);
    assert_eq!(shared.snapshot_len, digest.snapshot_len);
    assert!(shared.to_line().contains("crc45=0x"));
    assert!(
        !shared.to_line().contains("state_digest"),
            "the per-view snapshot CRC is not a cross-client value"
    );
}

/// H3: the local team is the slot of the own node, otherwise the requested one.
#[test]
fn local_team_comes_from_the_own_node_slot() {
    let lobby = LobbyInfo {
        player_node: 11,
        team_nodes: [10, 11, 12, 0],
        ..LobbyInfo::default()
    };
    assert_eq!(local_team_of(&lobby, 0), TeamId::GREEN);

    let lobby = LobbyInfo {
        player_node: 99,
        team_nodes: [10, 11, 0, 0],
        ..LobbyInfo::default()
    };
    assert_eq!(local_team_of(&lobby, 2), TeamId::BLUE);
}

/// Stage C §4.3 (A4): packet 12 is the only lobby setup command the world reads;
/// 13 is ignored (the seed comes from packet 30) and never reaches a dispatcher.
#[test]
fn lobby_collects_12_and_ignores_13() {
    let mut info = LobbyInfo::default();

    let supply = Packet12MissionSupply {
        team: TeamId::GREEN,
        round: 1,
        start_gold: 100,
        team_gold: 200,
        units: Vec::new(),
        gold_spent_on_upgrades: 0,
        starting_position: Point::new(55, 44),
        proximity_alert_ack: 0,
    };
    note_lobby_command(&mut info, Command::MissionSupply(supply.clone()));
    assert_eq!(
        info.mission_supplies[TeamId::GREEN.0 as usize],
        Some(supply)
    );
    assert_eq!(
        info.mission_supplies[TeamId::RED.0 as usize],
        None,
            "a supply is stored for its own team only"
    );

    // 13 is ignored: the world is generated from `LobbyGameSettings::rng_seed`.
    note_lobby_command(&mut info, Command::Seed(Packet13Seed { seed: 7 }));
    assert_eq!(info.mission_supplies[TeamId::RED.0 as usize], None);
}

/// The relay copy comes from the server that has no team: the transport
/// event carries only the payload. The frame goes through the real wire codec.
fn relayed(packet: &Packet) -> IoEvent {
    let frame = packet.encode().expect("encode barrier");
    IoEvent::Packet(Packet::decode(&frame).expect("decode relayed frame"))
}

/// Packet 00 from the server with `team = 1` is counted as team 1.
#[test]
fn relayed_00_from_the_server_counts_for_the_payload_team() {
    let local = TeamId::RED;
    let mut app = network_client(local, &[TeamId::GREEN], PlayMode::SimultaneousMoves);
    let mut stats = GameLoopStats::default();

    let packets = app.barrier_begin();
    assert_eq!(barrier_counter(&packets[0]), 1, "счётчики с 1");
    assert_eq!(app.state_ref().frame_sync_counter()[local.0 as usize], 1);
    assert!(!app.barrier_reached(), "пир ещё не ответил");

    let relayed_00 = Packet::Command(Command::TurnBarrier(Packet00TurnBarrier {
        team: TeamId::GREEN,
        counter: 1,
    }));
    app.handle_session_event(relayed(&relayed_00), &mut stats)
        .expect("релейный 00");

    assert_eq!(
        app.state_ref().frame_sync_counter()[TeamId::GREEN.0 as usize],
        1,
            "team берётся из payload, а не из ENet-отправителя (сервер)"
    );
    assert!(app.barrier_reached(), "00-барьер проходит по team=1");
}

/// The incoming 18 gets into the HUD log immediately and is **queued** for the
/// simulation: a peer command is replayed in receive order (A6), not applied in
/// the handler; the next tick writes `GameState::chat_log`.
#[test]
fn incoming_18_chat_is_queued_and_reaches_the_simulation_on_the_tick() {
    let local = TeamId::RED;
    let mut app = network_client(local, &[TeamId::GREEN], PlayMode::TurnBased);
    let mut stats = GameLoopStats::default();

    let incoming = Packet::Command(Command::Chat(Packet18Chat {
        sender: TeamId::GREEN,
        message: "hello".to_string(),
    }));

    app.handle_session_event(relayed(&incoming), &mut stats)
        .expect("входящий 18");

    assert_eq!(
        app.hud().log.lines().last().map(String::as_str),
        Some("GREEN: hello"),
            "the HUD line is the C++ `Remote_ReceiveNetPacket_18` format"
    );
    assert_eq!(
        app.state_ref().commands().len(),
        1,
            "A6: the peer command waits in the receive-order queue"
    );
    assert!(app.state_ref().chat_log().is_empty());

    app.tick();
    assert_eq!(
        app.state_ref().chat_log(),
        vec![(TeamId::GREEN, "hello".to_string())]
    );
    assert_eq!(app.state_ref().commands().len(), 0);
}

/// Gate (a) — the barrier path is packet-for-packet and counter-for-counter the
/// legacy bridge on the same `synthetic_universe` scenario: 00 → 53 → 52 → 54 →
/// 45 with the pre-incremented separate counters and the same payloads. The
/// 3 s/30 s timeouts and the resume-54 are covered by the `dispatcher` tests
/// (`barrier_pending_*`, `resume_state_sync_*`).
#[test]
fn gate_a_barrier_path_is_packet_for_packet_the_legacy_bridge() {
    let local = TeamId::RED;
    let peer = TeamId::GREEN;
    let mut app = network_client(local, &[peer], PlayMode::TurnBased);

    // 00 — `++Remote_FrameSyncCounter`, the first report is 1.
    let begin = app.barrier_begin();
    assert_eq!(
        begin,
        vec![Packet::Command(Command::TurnBarrier(Packet00TurnBarrier {
            team: local,
            counter: 1,
        }))]
    );
    assert_eq!(app.state_ref().frame_sync_counter()[local.0 as usize], 1);
    assert!(!app.barrier_reached(), "the peer has not answered yet");
    app.universe.set_frame_sync_counter(peer, 1);
    assert!(app.barrier_reached());

    // 53 — the 1-based `GameManager_TurnCounter` and the owner's totals.
    let play = app.barrier_complex();
    assert_eq!(play.len(), 1);
    assert!(matches!(
        &play[0],
        Packet::Command(Command::ComplexSummary(summary))
            if summary.turn == 1 && summary.team == local
    ));
    assert!(!app.barrier_reached());
    app.universe.dispatcher_mut().note_complex(peer, 1);
    assert!(app.barrier_reached());

    // 52 — a separate counter (`Remote_TurnIndices`), also 1.
    let end = app.barrier_end();
    assert_eq!(
        end,
        vec![Packet::Command(Command::EndTurnBarrier(
            Packet52EndTurnBarrier {
                team: local,
                counter: 1,
            }
        ))]
    );
    assert_eq!(app.state_ref().turn_indices()[local.0 as usize], 1);
    app.universe.set_turn_index(peer, 1);
    assert!(app.barrier_reached());

    // 54 — index 1, the own team's state.
    let sync = app.barrier_sync();
    assert!(matches!(
        &sync[0],
        Packet::StateSync(packet) if packet.sync_index == 1 && packet.team == local
    ));
    app.universe.set_state_sync_index(peer, 1);
    assert!(app.barrier_reached());

    // 45 — index 1, the own CRC after the 54 correction.
    let own_crc = app.state_ref().state_checksum();
    let crc = app.barrier_crc();
    assert_eq!(barrier_counter(&crc[0]), 1);
    assert!(matches!(
        &crc[0],
        Packet::Command(Command::CrcBarrier(barrier)) if barrier.crc16 == own_crc
    ));
    app.universe.set_next_turn_index(peer, 1);
    app.universe.set_team_data_crc16(peer, own_crc);
    assert!(app.barrier_reached());
    assert!(app.crc_matches());
}

/// Gate (b) — `submit(Broadcast)` → `drain_outgoing`; `Local`/`Drop` never reach
/// the transport, and the outgoing plan is **extended** by the following tick
/// (A2), then drained once. `broadcast_outgoing` sends exactly these DTOs as
/// `Packet::Command`, i.e. the same frames the relayed tests deliver to peers.
#[test]
fn gate_b_broadcast_reaches_drain_outgoing_and_local_drop_do_not() {
    let local = TeamId::RED;
    let mut app = network_client(local, &[TeamId::GREEN], PlayMode::TurnBased);

    let first = Command::FinishedTurn(Packet06FinishedTurn { team: local });
    let second = Command::Chat(Packet18Chat {
        sender: local,
        message: "hi".to_string(),
    });
    assert_eq!(app.submit(first.clone()), SubmitOutcome::Accepted);
    assert_eq!(app.submit(second.clone()), SubmitOutcome::Accepted);

    // A barrier is `Local`, a mid-match 13 is `Drop` — neither is broadcast.
    let barrier = Command::TurnBarrier(Packet00TurnBarrier {
        team: local,
        counter: 1,
    });
    assert_eq!(app.submit(barrier.clone()), SubmitOutcome::Accepted);
    assert_eq!(
        app.submit(Command::Seed(Packet13Seed { seed: 5 })),
        SubmitOutcome::Dropped
    );

    // The tick extends the plan instead of replacing it (`outgoing` keeps the
    // submitted packets), and the drain takes everything exactly once.
    app.tick();
    assert_eq!(
        app.drain_outgoing(),
        vec![
            NetworkCommand::Command(first),
            NetworkCommand::Command(second)
        ]
    );
    assert!(app.drain_outgoing().is_empty(), "drained once");
}

/// State for a three-client scenario: one tank per team. The world is
/// deterministic, so each client rebuilds it with its own dispatcher.
fn three_team_state<D: max_core::universe::Dispatcher>(
    dispatcher: D,
) -> (max_core::universe::GameUniverse<D>, [max_core::UnitId; 3]) {
    use max_core::geo::Point;

    let mut state = max_core::universe::GameUniverse::with_dispatcher(dispatcher);
    let teams = [TeamId::RED, TeamId::GREEN, TeamId::BLUE];
    let mut ids: [max_core::UnitId; 3] = [max_core::UnitId::NO_ID; 3];

    for (index, team) in teams.iter().copied().enumerate() {
        *state.team_units_mut(team).expect("team economy") =
            TeamUnits::with_clan(TeamClan::Random);
        ids[index] = state
            .spawn_unit(
                UnitType::Tank,
                team,
                Point::new(20 + index as i16 * 5, 20),
                0,
            )
            .expect("tank spawns");
    }
    state.set_play_mode(PlayMode::SimultaneousMoves);
    (state, ids)
}

struct SimClient {
    app: App<NetworkDispatcher>,
}

impl SimClient {
    fn new(local: TeamId, remotes: &[TeamId]) -> Self {
        let (mut universe, _) =
            three_team_state(NetworkDispatcher::new(local, PlayMode::SimultaneousMoves));
        for team in 0..PLAYER_TEAM_COUNT {
            if let Some(info) = universe.team_mut(TeamId(team as u8)) {
                info.team_type = TeamKind::None;
            }
        }
        if let Some(info) = universe.team_mut(local) {
            info.team_type = TeamKind::Player;
        }
        for team in remotes {
            if let Some(info) = universe.team_mut(*team) {
                info.team_type = TeamKind::Remote;
            }
        }
        universe.set_player_team(local);

        let mut app = App::<NetworkDispatcher>::new_network(
            AppConfig::default(),
            local,
            PlayMode::SimultaneousMoves,
        );
        app.universe = universe;
        Self { app }
    }
}

/// Relay in a "star": the sender's packet is received by all other clients.
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
                client
                    .app
                    .handle_session_event(relayed(packet), &mut stats[target])
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

/// P5: three clients, B disconnects, recovery via 54 and the turn continues.
///
/// Mock transport — a deterministic relay without the network: while B is disconnected,
/// its packets are not delivered; A/C have time to "move" and spoil B's unit
/// copy (like a missed order). After reconnecting, B publishes its 54 —
/// A/C accept the owner's state; the peer 54 of A corrects the copy
/// of A's unit at B. The turn reaches 45, the CRC and turn_counter match.
#[test]
fn three_clients_survive_disconnect_and_converge_via_state_sync() {
    let (_, ids) = three_team_state(NetworkDispatcher::new(
        TeamId::RED,
        PlayMode::SimultaneousMoves,
    ));
    let mut clients = vec![
        SimClient::new(TeamId::RED, &[TeamId::GREEN, TeamId::BLUE]),
        SimClient::new(TeamId::GREEN, &[TeamId::RED, TeamId::BLUE]),
        SimClient::new(TeamId::BLUE, &[TeamId::RED, TeamId::GREEN]),
    ];
    let mut stats = vec![
        GameLoopStats::default(),
        GameLoopStats::default(),
        GameLoopStats::default(),
    ];

    // 00: all three.
    send_all(&mut clients, &mut stats, |client| {
        client.app.barrier_begin()
    });
    assert!(
        clients.iter().all(|client| client.app.barrier_reached()),
            "00 с 3 участниками"
    );

    // 53: all three.
    send_all(&mut clients, &mut stats, |client| {
        client.app.barrier_complex()
    });
    assert!(
        clients.iter().all(|client| client.app.barrier_reached()),
            "53 с 3 участниками"
    );

    // 06: local end of the turn, relay to the others.
    let mut finish_packets = Vec::new();
    for client in &mut clients {
        let finish = Command::FinishedTurn(Packet06FinishedTurn {
            team: client.app.universe.dispatcher().local_team(),
        });
        client.app.apply_immediate(finish.clone());
        client
            .app
            .universe
            .dispatcher_mut()
            .mark_local_finished();
        finish_packets.push(vec![Packet::Command(finish)]);
    }
    let senders: Vec<usize> = (0..clients.len()).collect();
    relay(&finish_packets, &senders, &mut clients, &mut stats);

    // B (GREEN) disappears: A and C send 52, but B does not receive them, and its 52
    // is lost. Without B the barrier does not close — this is lockstep.
    let mut packets: Vec<Vec<Packet>> = Vec::new();
    for client in &mut clients {
        packets.push(client.app.barrier_end());
    }
    relay(&packets, &[0, 2], &mut clients, &mut stats);
    assert!(!clients[0].app.barrier_reached());
    assert!(!clients[2].app.barrier_reached());

    // While B is away, A/C change the copy of B's tank (a missed attack order).
    for index in [0usize, 2] {
        clients[index]
            .app
            .universe
            .unit_mut(ids[1])
            .expect("tank B")
            .hits = 1;
    }
    // And B's copy of A's tank is stale.
    clients[1]
        .app
        .universe
        .unit_mut(ids[0])
        .expect("tank A")
        .hits = 2;
    let b_tank_hits = clients[1]
        .app
        .state_ref()
        .unit(ids[1])
        .expect("tank B")
        .hits;
    let a_tank_hits = clients[0]
        .app
        .state_ref()
        .unit(ids[0])
        .expect("tank A")
        .hits;

    // Reconnect: B publishes its 54 (sync_index still 0), A/C apply it.
    let resume = clients[1]
        .app
        .universe
        .dispatcher()
        .resume_state_sync(&clients[1].app.universe);
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
                .state_ref()
                .unit(ids[1])
                .expect("tank B")
                .hits,
            b_tank_hits,
                "54 владельца восстановил копию танка B"
        );
    }

    // B repeats its lost 52 (the same counter, without a new
    // pre-increment); A/C were already waiting for exactly it.
    let mut end_packets = vec![Vec::new(); clients.len()];
    end_packets[1] = packets[1].clone();
    relay(&end_packets, &[1], &mut clients, &mut stats);
    assert!(
        clients.iter().all(|client| client.app.barrier_reached()),
            "52 после возврата B: {:?}",
        clients
            .iter()
            .map(|client| (
                client.app.universe.dispatcher().local_team().0,
                client.app.universe.dispatcher().end_counter(),
                client.app.state_ref().turn_indices(),
            ))
            .collect::<Vec<_>>()
    );

    // 54 of all teams: B gets the current state of A/C, A/C — of B.
    send_all(&mut clients, &mut stats, |client| client.app.barrier_sync());
    assert!(
        clients.iter().all(|client| client.app.barrier_reached()),
            "54 с 3 участниками"
    );
    assert_eq!(
        clients[1]
            .app
            .state_ref()
            .unit(ids[0])
            .expect("tank A")
            .hits,
        a_tank_hits,
            "54 A исправил устаревшую копию танка A у B"
    );

    // 45: each client's CRC matches everyone's.
    send_all(&mut clients, &mut stats, |client| client.app.barrier_crc());
    assert!(
        clients.iter().all(|client| client.app.barrier_reached()),
            "45 с 3 участниками"
    );
    for client in &clients {
        assert!(client.app.crc_matches(), "CRC 45");
    }

    let checksums: Vec<u16> = clients
        .iter()
        .map(|client| client.app.state_ref().state_checksum())
        .collect();
    assert_eq!(checksums[0], checksums[1], "CRC A == B");
    assert_eq!(checksums[0], checksums[2], "CRC A == C");

    for client in &mut clients {
        client.app.universe.enter_end_turn();
        let _ = client.app.universe.finish_turn();
    }
    let turns: Vec<i32> = clients
        .iter()
        .map(|client| client.app.state_ref().turn_counter())
        .collect();
    assert_eq!(turns, vec![1, 1, 1], "turn_counter догнал у всех");
}

/// Two peers, the relay packets come from the server — barrier 00 converges.
#[test]
fn two_peers_pass_the_relayed_begin_barrier() {
    let mut apps = [
        network_client(TeamId::RED, &[TeamId::GREEN], PlayMode::SimultaneousMoves),
        network_client(TeamId::GREEN, &[TeamId::RED], PlayMode::SimultaneousMoves),
    ];
    let mut stats = GameLoopStats::default();

    let red = apps[0].barrier_begin();
    let green = apps[1].barrier_begin();
    assert_eq!(barrier_counter(&red[0]), 1);
    assert_eq!(barrier_counter(&green[0]), 1);

    // The server relays 00(RED) to peer GREEN and 00(GREEN) to peer RED.
    apps[1]
        .handle_session_event(relayed(&red[0]), &mut stats)
        .expect("00 RED → GREEN");
    apps[0]
        .handle_session_event(relayed(&green[0]), &mut stats)
        .expect("00 GREEN → RED");

    assert!(
        apps[0].barrier_reached(),
            "RED ждал FrameSyncCounter[GREEN] == 1"
    );
    assert!(
        apps[1].barrier_reached(),
            "GREEN ждал FrameSyncCounter[RED] == 1"
    );
    assert_eq!(
        apps[0].state_ref().frame_sync_counter()[TeamId::GREEN.0 as usize],
        1
    );
    assert_eq!(
        apps[1].state_ref().frame_sync_counter()[TeamId::RED.0 as usize],
        1
    );
}

/// A disconnect in the event handler alone does not panic and ends
/// the run (the retry is done by `pump_events`; the fallback is checked here).
#[test]
fn disconnect_event_stops_the_handler_without_panicking() {
    let local = TeamId::RED;
    let mut app = network_client(local, &[TeamId::GREEN], PlayMode::TurnBased);
    let mut stats = GameLoopStats::default();

    let keep_going = app
        .handle_session_event(IoEvent::Disconnected, &mut stats)
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

    // A new disconnect — a new round, the previous error is not carried over.
    tracker.note_disconnect();
    assert_eq!(tracker.attempts(), 0);
    assert!(tracker.last_error().is_none());
    assert_eq!(tracker.next_attempt(), Some(1));
}

/// Mock session: the first attempt fails, the second succeeds — the retry logic
/// restores the transport and reports the number of the successful attempt.
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

/// All attempts fail: `Exhausted` with the attempt count, without panics.
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

/// `attempts = 0` — reconnect is disabled, the connector is not called.
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

/// An attempt timeout does not hang the run and goes into `last_error`.
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
// M-NET follow-ups: first-connection retry and data_hash fallback
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

/// Mock connector: the first attempt fails, the second succeeds. Time is
/// controlled by `start_paused`: between attempts exactly backoff (1 s).
#[tokio::test(start_paused = true)]
async fn join_retry_succeeds_on_the_second_attempt_after_backoff() {
    let started = tokio::time::Instant::now();
    let mut calls = 0u32;

    let outcome = retry_join(
        join_policy(3, Duration::from_secs(2)),
        |_| {},
        |attempt| {
            calls += 1;
            async move {
                if attempt == 1 {
                    Err(JoinAttemptError::Timeout(Duration::from_secs(2)))
                } else {
                    Ok(42u32)
                }
            }
        },
    )
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

/// The attempt timeout is bounded by the policy and does not hang the run: 2 s + 1 s
/// backoff + 2 s = 5 s of virtual time for two attempts.
#[tokio::test(start_paused = true)]
async fn join_retry_attempt_timeout_is_bounded_by_the_policy() {
    let started = tokio::time::Instant::now();

    let outcome = retry_join(
        join_policy(2, Duration::from_secs(2)),
        |_| {},
        |_attempt| async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok::<(), JoinAttemptError>(())
        },
    )
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

/// `TP 03 GameData` → one reconnect with the MAX.RES-only hash (the same
/// ticket), the session starts. It is the full hash that is rejected.
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

/// A refusal for another reason (`Version`) — no fallback and no retry.
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

/// The network loop waits for the `PATCHES.RES` variant only without an explicit flag.
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

    // Explicit --handshake-patches — exactly one variant, no guessing.
    let explicit = AppConfig {
        handshake_patches: Some(dir.join("nonexistent.res")),
        ..config.clone()
    };
    let explicit_hashes = handshake_data_hashes(&explicit).expect("хэши");
    assert_eq!(explicit_hashes.len(), 1);
    assert!(explicit_hashes[0].patches_res.is_none());

    // No local PATCHES.RES — the full variant is already MAX-only.
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

// -----------------------------------------------------------------------
// H5: desync audit 23/24/49 (`remote.cpp:880-1428`)
// -----------------------------------------------------------------------

/// A scripted transport: records every sent packet (APPL or transport frame)
/// and replays a fixed event script. Used by the audit round tests.
struct ScriptedIo {
    sent: Vec<Packet>,
    script: std::collections::VecDeque<IoEvent>,
    authority: bool,
}

#[allow(async_fn_in_trait)]
impl HostIo for ScriptedIo {
    async fn send_packet(&mut self, packet: Packet) -> Result<()> {
        self.sent.push(packet);
        Ok(())
    }

    async fn send_packet_untracked(&mut self, packet: Packet) -> Result<()> {
        self.sent.push(packet);
        Ok(())
    }

    async fn clear_barriers(&mut self) -> Result<()> {
        Ok(())
    }

    async fn next_io_event(&mut self, _wait: Duration) -> Result<IoEvent> {
        // `Closed` after the script: an audit must never spin forever.
        Ok(self.script.pop_front().unwrap_or(IoEvent::Closed))
    }

    async fn on_disconnect(&mut self) -> Result<DisconnectOutcome> {
        Ok(DisconnectOutcome::Ignored)
    }

    fn is_authority(&self) -> bool {
        self.authority
    }
}

fn temp_audit_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
            "max-brh-audit-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("audit temp dir");
    dir
}

/// Host round: one 23 per non-mining unit, one 24 per peer, then 49; the
/// `status=false` bits become the `desync-turn<turn>.txt` mismatch list.
#[tokio::test]
async fn desync_audit_host_sends_23_per_unit_aggregates_24_and_writes_a_report() {
    let mut client = SimClient::new(TeamId::RED, &[TeamId::GREEN, TeamId::BLUE]);
    let units: Vec<UnitId> = client.app.universe.snapshot().unit_ids_in_order();
    assert_eq!(units.len(), 3, "the audit covers all three tanks");

    let mut script = std::collections::VecDeque::new();
    for (index, _unit) in units.iter().enumerate() {
        script.push_back(IoEvent::Packet(Packet::AuditAck {
            team: TeamId::GREEN,
            status: index != 0,
        }));
        script.push_back(IoEvent::Packet(Packet::AuditAck {
            team: TeamId::BLUE,
            status: true,
        }));
    }
    let mut io = ScriptedIo {
        sent: Vec::new(),
        script,
        authority: true,
    };

    let dir = temp_audit_dir("host");
    let mut stats = GameLoopStats::default();
    let report = client
        .app
        .run_host_desync_audit(&mut io, &mut stats, 3, Some(&dir))
        .await
        .expect("host audit");

    assert_eq!(report.turn, 3);
    assert_eq!(report.units, 3);
    assert_eq!(report.peers, vec![TeamId::GREEN, TeamId::BLUE]);
    assert_eq!(
        report.mismatches,
        vec![AuditMismatch {
            unit: units[0],
            team: TeamId::GREEN
        }],
            "only the first unit differs on GREEN"
    );
    assert_eq!(report.missing_acks, 0);
    assert!(!report.aborted);

    let audit_packets = io
        .sent
        .iter()
        .filter(|packet| matches!(packet, Packet::Audit { .. }))
        .count();
    assert_eq!(audit_packets, 3, "one 23 per unit");
    assert!(
        matches!(io.sent.last(), Some(Packet::AuditDone { param: 1, .. })),
            "49 closes the round: {:?}",
        io.sent.last()
    );

    let path = report.report.clone().expect("report path");
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("desync-turn3.txt")
    );
    let text = std::fs::read_to_string(&path).expect("report text");
    assert!(text.contains("mismatch: unit"), "{text}");
    assert!(text.contains("units audited: 3"), "{text}");
    assert!(
        report.to_line().contains("mismatches=1"),
            "{}",
        report.to_line()
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Guest round: compares each 23 with the local world, answers 24 and does
/// not write a report (the authority aggregates).
#[tokio::test]
async fn desync_audit_guest_answers_23_with_24_and_records_its_mismatches() {
    let mut client = SimClient::new(TeamId::GREEN, &[TeamId::RED, TeamId::BLUE]);
    let units: Vec<UnitId> = client.app.universe.snapshot().unit_ids_in_order();
    let snapshot = client.app.universe.snapshot();

    let mut corrupted = UnitState::from_unit(snapshot, snapshot.unit(units[0]).expect("unit"));
    corrupted.hits = corrupted.hits.wrapping_add(1);
    let mut script = std::collections::VecDeque::new();
    script.push_back(IoEvent::Packet(Packet::Audit {
        unit: units[0],
        state: corrupted,
    }));
    for unit in units.iter().skip(1) {
        let state = UnitState::from_unit(snapshot, snapshot.unit(*unit).expect("unit"));
        script.push_back(IoEvent::Packet(Packet::Audit { unit: *unit, state }));
    }
    script.push_back(IoEvent::Packet(Packet::AuditDone {
        team: TeamId::RED,
        param: 1,
    }));

    let mut io = ScriptedIo {
        sent: Vec::new(),
        script,
        authority: false,
    };
    let mut stats = GameLoopStats::default();
    let report = client
        .app
        .wait_desync_audit(&mut io, &mut stats, 2)
        .await
        .expect("guest audit");

    assert_eq!(report.units, 3);
    assert_eq!(
        report.mismatches,
        vec![AuditMismatch {
            unit: units[0],
            team: TeamId::GREEN
        }]
    );
    assert!(!report.aborted);
    assert!(report.report.is_none(), "only the host writes the report");
    assert_eq!(io.sent.len(), 3, "one 24 per audited unit");
    assert!(io
        .sent
        .iter()
        .all(|packet| matches!(packet, Packet::AuditAck { .. })));
    assert_eq!(
        io.sent[0],
        Packet::AuditAck {
            team: TeamId::GREEN,
            status: false
        }
    );
    assert_eq!(
        io.sent[1],
        Packet::AuditAck {
            team: TeamId::GREEN,
            status: true
        }
    );
}
