use super::*;
use max_core::input::{Command, Packet18Chat};
use max_net::DataHash;

fn config(min_players: usize) -> HostLobbyConfig {
    HostLobbyConfig {
        host_name: "Host".to_string(),
        host_team: 0,
        min_players,
        policy: SessionPolicy::Pause,
        settings: HostSessionSettings {
            seed: 0x0102_0304,
            ..HostSessionSettings::default()
        }
        .lobby_settings(),
    }
}

fn lobby(min_players: usize) -> HostLobby {
    HostLobby::new(config(min_players))
}

fn packet(packet: LobbyPacket) -> Packet {
    Packet::Lobby(packet)
}

/// 28 of a verified peer (`Remote_ReceiveNetPacket_28`).
fn enter(lobby: &mut HostLobby, peer: u32) -> Vec<LobbyOutgoing> {
    lobby.on_verified(peer);
    lobby.on_packet(peer, &packet(LobbyPacket::EnterLobby { node: 0 }))
}

/// 29 of a peer from packet 44's `host_node`.
fn register(lobby: &mut HostLobby, peer: u32) -> Vec<LobbyOutgoing> {
    lobby.on_packet(
        peer,
        &packet(LobbyPacket::Register {
            host_node: HOST_NODE_ID,
        }),
    )
}

/// The node id packet 30 assigned to the peer.
fn welcome_node(actions: &[LobbyOutgoing]) -> u16 {
    actions
        .iter()
        .find_map(|action| match &action.packet {
            LobbyPacket::Welcome(welcome) => Some(welcome.player_node),
            _ => None,
        })
        .expect("29 must produce a welcome")
}

fn team_select(
    lobby: &mut HostLobby,
    peer: u32,
    node: u16,
    slot: u8,
    ready: bool,
) -> Vec<LobbyOutgoing> {
    lobby.on_packet(
        peer,
        &packet(LobbyPacket::TeamSelect {
            host_node: HOST_NODE_ID,
            player_node: node,
            team_slot: slot,
            ready,
            team_name: format!("Guest#{slot}"),
        }),
    )
}

/// 28 → 44 then 29 → 47 + 30, with node ids following the entry order.
#[test]
fn enter_and_register_assign_nodes_in_entry_order() {
    let mut session = lobby(2);

    let actions = enter(&mut session, 7);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].target, LobbyTarget::Source);
    match &actions[0].packet {
        LobbyPacket::HostAnnounce {
            player_node,
            game_version,
            player_name,
        } => {
            assert_eq!(*player_node, HOST_NODE_ID);
            assert_eq!(*game_version, DEFAULT_GAME_VERSION);
            assert_eq!(player_name, "Host");
        }
        other => panic!("28 must be answered with 44, got {other:?}"),
    }

    let actions = register(&mut session, 7);
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0].target, LobbyTarget::Broadcast);
    match &actions[0].packet {
        LobbyPacket::NodeRegistered { player_node, node } => {
            assert_eq!(*player_node, HOST_NODE_ID);
            assert_eq!(node.entity_id, FIRST_CLIENT_NODE);
            assert!(!node.is_host);
        }
        other => panic!("29 must produce 47, got {other:?}"),
    }

    let LobbyPacket::Welcome(welcome) = &actions[1].packet else {
        panic!("29 must produce 30");
    };
    assert_eq!(actions[1].target, LobbyTarget::Source);
    assert_eq!(welcome.player_node, FIRST_CLIENT_NODE);
    assert_eq!(welcome.host_node, HOST_NODE_ID);
    assert_eq!(welcome.team_nodes, [HOST_NODE_ID, 0, 0, 0]);
    assert_eq!(welcome.team_names[0].as_str(), "Host");
    assert_eq!(welcome.nodes.len(), 2);
    assert!(welcome.nodes[0].is_host);
    assert_eq!(welcome.nodes[0].entity_id, HOST_NODE_ID);
    assert_eq!(welcome.nodes[1].entity_id, FIRST_CLIENT_NODE);
    assert_eq!(welcome.settings, config(2).settings);

    // The second guest gets the next node id, not the same one.
    enter(&mut session, 9);
    let actions = register(&mut session, 9);
    let LobbyPacket::Welcome(second) = &actions[1].packet else {
        panic!("29 must produce 30");
    };
    assert_eq!(second.player_node, FIRST_CLIENT_NODE + 1);
    assert_eq!(second.nodes.len(), 3);
}

/// READY ≥ `min_players` broadcasts 32; the guest's 05 broadcasts 34.
#[test]
fn ready_players_reach_32_and_the_acknowledgement_reaches_34() {
    let mut session = lobby(2);
    enter(&mut session, 7);
    let actions = register(&mut session, 7);
    let node = welcome_node(&actions);

    let actions = team_select(&mut session, 7, node, 1, true);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].target, LobbyTarget::Broadcast);
    assert!(matches!(
        &actions[0].packet,
        LobbyPacket::Setup { host_node } if *host_node == HOST_NODE_ID
    ));
    assert!(session.status().setup_sent);
    assert_eq!(session.status().pending_acks, 1);

    let actions = session.on_packet(
        7,
        &packet(LobbyPacket::SetupAck {
            host_node: HOST_NODE_ID,
        }),
    );
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].target, LobbyTarget::Broadcast);
    assert!(matches!(
        &actions[0].packet,
        LobbyPacket::Start { host_node } if *host_node == HOST_NODE_ID
    ));
    assert!(session.is_complete());

    let info = session.lobby_info();
    assert_eq!(info.player_node, HOST_NODE_ID);
    assert_eq!(info.host_node, HOST_NODE_ID);
    assert_eq!(info.team_nodes, [HOST_NODE_ID, node, 0, 0]);
    assert_eq!(
        info.settings.as_ref().expect("settings").rng_seed,
        0x0102_0304
    );
    assert!(info.summary().contains("team_nodes=[10, 11, 0, 0]"));
}

/// The host slot cannot be stolen, and a node mismatch is rejected.
#[test]
fn team_select_rejects_the_host_slot_and_a_foreign_node() {
    let mut session = lobby(2);
    enter(&mut session, 7);
    let actions = register(&mut session, 7);
    let node = welcome_node(&actions);

    let actions = team_select(&mut session, 7, node, 0, true);
    assert!(actions.is_empty());
    assert_eq!(session.lobby_info().team_nodes[0], HOST_NODE_ID);
    assert_eq!(session.status().ready_players, 1);

    let actions = team_select(&mut session, 7, node + 5, 1, true);
    assert!(actions.is_empty());
    assert!(!session.status().setup_sent);
}

/// One READY guest is not enough when `--min-players` is 3.
#[test]
fn the_ready_threshold_is_inclusive_of_the_host() {
    let mut session = lobby(3);
    enter(&mut session, 7);
    let actions = register(&mut session, 7);
    let node = welcome_node(&actions);

    let actions = team_select(&mut session, 7, node, 1, true);
    assert!(actions.is_empty(), "2 of 3 players must not start");
    assert_eq!(session.status().ready_players, 2);
    assert!(!session.status().setup_sent);
}

/// A disconnect frees the jar; the remaining host slot is not overwritten.
#[test]
fn a_disconnect_clears_the_team_slot() {
    let mut session = lobby(3);
    enter(&mut session, 7);
    let actions = register(&mut session, 7);
    let node = welcome_node(&actions);
    session.on_packet(
        7,
        &packet(LobbyPacket::TeamSelect {
            host_node: HOST_NODE_ID,
            player_node: node,
            team_slot: 2,
            ready: true,
            team_name: "Guest".to_string(),
        }),
    );

    let actions = session.on_disconnect(7);
    assert!(actions.is_empty());
    assert_eq!(session.lobby_info().team_nodes, [HOST_NODE_ID, 0, 0, 0]);
    assert_eq!(session.status().peers, 0);
}

/// Packet 26 stores the clan by team slot (`Remote_ReceiveNetPacket_26`).
#[test]
fn clan_packets_are_stored_per_team_slot() {
    let mut session = lobby(2);
    session.on_verified(7);

    let actions = session.on_packet(
        7,
        &packet(LobbyPacket::Clan {
            team: TeamId(2),
            clan: TeamClan::Musashi,
        }),
    );

    assert!(actions.is_empty());
    assert_eq!(session.team_clan(2), Some(TeamClan::Musashi));
    assert_eq!(session.team_clan(0), None);
}

/// `--host-policy` names.
#[test]
fn session_policy_parses_the_cli_values() {
    assert_eq!(SessionPolicy::parse("pause"), Some(SessionPolicy::Pause));
    assert_eq!(SessionPolicy::parse("PAUSE"), Some(SessionPolicy::Pause));
    assert_eq!(
        SessionPolicy::parse("eliminate"),
        Some(SessionPolicy::EliminateAfter {
            ticks: DEFAULT_ELIMINATION_TICKS
        })
    );
    assert_eq!(SessionPolicy::parse("other"), None);
    assert_eq!(SessionPolicy::default().to_string(), "pause");
    assert_eq!(
        SessionPolicy::EliminateAfter { ticks: 1 }.to_string(),
            "eliminate"
    );
}

/// Gate H2 invariant: the host's `LobbyInfo` and the guest's `Welcome`
/// (with its own slot patched locally, as `run_lobby` does) have the same
/// identity line.
#[test]
fn host_and_guest_lobby_identity_agree() {
    let mut session = lobby(2);
    enter(&mut session, 7);
    let actions = register(&mut session, 7);
    let node = welcome_node(&actions);
    let welcome = actions
        .iter()
        .find_map(|action| match &action.packet {
            LobbyPacket::Welcome(welcome) => Some((**welcome).clone()),
            _ => None,
        })
        .expect("welcome");

    team_select(&mut session, 7, node, 1, true);

    // Guest side of `run_lobby`: `Welcome`, own team slot patched locally.
    let mut guest_team_nodes = welcome.team_nodes;
    guest_team_nodes[1] = node;
    let guest_info = LobbyInfo {
        handshake: true,
        player_node: node,
        host_node: welcome.host_node,
        team_nodes: guest_team_nodes,
        settings: Some(welcome.settings.clone()),
        ..LobbyInfo::default()
    };

    assert_eq!(session.lobby_info().summary(), guest_info.summary());
}

/// H3: the host's `HostIo` adapter maps transport events and keeps the match
/// alive when a guest is lost (`DisconnectOutcome::Ignored`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_io_adapter_is_idle_and_ignores_a_lost_guest() {
    let socket = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("probe socket");
    let port = socket.local_addr().expect("probe addr").port();
    drop(socket);

    let config = HostConfig::new(
        Ipv4Addr::LOCALHOST,
        port,
        DataHash::from_bytes(b"max", b"patches"),
    );
    let mut host = HostHandle::bind(config).expect("bind host");

    assert!(
        matches!(
            host.next_io_event(Duration::from_millis(50))
                .await
                .expect("host event"),
            IoEvent::Idle
        ),
            "an empty server has no game event"
    );
    host.clear_barriers().await.expect("clear barriers");
    // The host is not a peer: broadcasting with an empty roster is a no-op.
    host.send_packet(Packet::Command(Command::Chat(Packet18Chat {
        sender: TeamId::RED,
        message: "no guests yet".to_string(),
    })))
    .await
    .expect("broadcast with no verified peers");
    assert_eq!(
        host.on_disconnect().await.expect("disconnect outcome"),
        DisconnectOutcome::Ignored,
            "H3 keeps waiting for a lost guest; H4 owns the session policy"
    );

    host.shutdown().await;
}
