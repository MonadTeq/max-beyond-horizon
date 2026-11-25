use super::*;
use max_net::{DataHash, HOST_TAKEOVER_PACKET_NUMBER, HOST_TIMEOUT_MAX, HOST_TIMEOUT_MIN};

fn roster(nodes: [u16; PLAYER_TEAM_COUNT], local: u16, host: u16) -> P2pRoster {
    P2pRoster::new(local, host, nodes)
}

fn policy(ticks: u32) -> SessionPolicy {
    SessionPolicy::EliminateAfter { ticks }
}

fn heartbeat(
    tick: u32,
    last_seen: [u32; PLAYER_TEAM_COUNT],
    eliminated: u32,
    eliminated_tick: [u32; PLAYER_TEAM_COUNT],
    quorum: u8,
) -> HostHeartbeat {
    HostHeartbeat {
        host_epoch: 1,
        session_tick: tick,
        turn_counter: 3,
        phase: 2,
        reachable: 0b11,
        last_seen,
        policy: HostSessionPolicy::EliminateAfter { ticks: 4 },
        host_timeout_ms: 90_000,
        quorum,
        eliminated,
        eliminated_tick,
    }
}

fn test_guest(nodes: [u16; PLAYER_TEAM_COUNT], local: u16, port: u16) -> P2pTransport {
    P2pTransport::with_link(
        None,
        SessionConfig::new(
                "127.0.0.1",
            port,
                "",
            DataHash::from_bytes(b"max", b"patches"),
        ),
        DirectEndpoint {
            host: "127.0.0.1".to_string(),
            port,
        },
        roster(nodes, local, 10),
        TeamId(if local == 11 { 1 } else { 2 }),
        SessionPolicy::Pause,
        HOST_TIMEOUT_MIN,
        ReconnectPolicy::default(),
    )
}

fn free_udp_port() -> u16 {
    let socket = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("probe socket");
    socket.local_addr().expect("probe addr").port()
}

fn takeover_proposal(node: u16, epoch: u32) -> Takeover {
    Takeover {
        kind: TakeoverKind::Proposal,
        epoch,
        turn_counter: 5,
        phase: phase_tag(BarrierPhase::Play),
        counters: TakeoverCounters {
            begin: 5,
            end: 5,
            sync: 5,
            crc: 4,
        },
        node,
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
    }
}

/// Election: `min(player_node)` among the survivors, never the dead host.
#[test]
fn successor_is_the_minimum_surviving_player_node() {
    let three = roster([10, 11, 12, 0], 11, 10);
    assert_eq!(three.successor(), Some(11));
    assert!(three.is_successor());
    assert_eq!(three.quorum(), 2);

    let other = roster([10, 11, 12, 0], 12, 10);
    assert_eq!(other.successor(), Some(11));
    assert!(!other.is_successor());

    // If only the higher survivor exists, it is the candidate; if the
    // candidate itself died nothing is elected (safe pause).
    let two = roster([10, 0, 12, 0], 12, 10);
    assert_eq!(two.successor(), Some(12));
    assert!(two.is_successor());
    assert_eq!(two.quorum(), 2);
}

/// Quorum is `floor(N_active/2)+1` for 2, 3 and 4 players.
#[test]
fn quorum_is_the_plan_rule() {
    assert_eq!(roster([10, 11, 0, 0], 10, 10).quorum(), 2);
    assert_eq!(roster([10, 11, 12, 0], 10, 10).quorum(), 2);
    assert_eq!(roster([10, 11, 12, 13], 10, 10).quorum(), 3);
}

/// Split-brain safety: a live host holds the port, so the candidate does not
/// bump the epoch; both sides stay at the barrier and a healed heartbeat
/// resumes the match.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn split_brain_never_bumps_the_epoch_and_resumes_on_merge() {
    let port = free_udp_port();
    // The live authority still owns the session port.
    let live = HostHandle::bind(HostConfig {
        bind_ip: Ipv4Addr::LOCALHOST,
        port,
        heartbeat: Duration::ZERO,
        host_timeout: HOST_TIMEOUT_MIN,
        ..HostConfig::new(
            Ipv4Addr::LOCALHOST,
            port,
            DataHash::from_bytes(b"max", b"patches"),
        )
    })
    .expect("live host");

    let mut transport = test_guest([10, 11, 12, 0], 11, port);
    transport.enter_failure().expect("failure state");
    // The probe says "dead", but the live authority still holds the port:
    // binding a second authority fails and the epoch is not bumped.
    assert!(!transport
        .try_start_takeover(false)
        .await
        .expect("bind attempt"));
    assert_eq!(transport.epoch(), 1, "the epoch is not bumped");
    assert!(matches!(transport.mode, P2pMode::HostFailure));

    // A healed heartbeat (same epoch) cancels the episode.
    transport.on_heartbeat(heartbeat(5, [5, 5, 5, 0], 0, [0; 4], 2));
    assert!(matches!(transport.mode, P2pMode::Guest));
    assert_eq!(transport.epoch(), 1);

    live.shutdown().await;
}

/// A takeover whose lockstep position differs is refused with `Reject` and
/// freezes the survivor (STOP), while a matching one is accepted.
#[tokio::test]
async fn takeover_with_a_mismatched_lockstep_position_is_refused() {
    let port = free_udp_port();
    let mut transport = test_guest([10, 11, 12, 0], 12, port);
    transport.note_lockstep(LockstepPosition {
        turn_counter: 5,
        phase: phase_tag(BarrierPhase::Play),
        begin: 5,
        end: 5,
        sync: 5,
        crc: 4,
    });

    let mut proposal = takeover_proposal(11, 2);
    proposal.turn_counter = 4; // different lockstep point
    transport.on_takeover_frame(proposal).await.unwrap();
    assert!(matches!(transport.mode, P2pMode::Paused));
    assert!(transport.rejected);

    let mut transport = test_guest([10, 11, 12, 0], 12, free_udp_port());
    transport.note_lockstep(LockstepPosition {
        turn_counter: 5,
        phase: phase_tag(BarrierPhase::Play),
        begin: 5,
        end: 5,
        sync: 5,
        crc: 4,
    });
    transport
        .on_takeover_frame(takeover_proposal(11, 2))
        .await
        .unwrap();
    assert!(matches!(transport.mode, P2pMode::AwaitingTakeover));
    assert!(transport.accepted_takeover);
    assert_eq!(transport.epoch(), 2);
}

/// The candidate only resumes with the quorum of survivor acceptances.
#[tokio::test]
async fn takeover_requires_the_quorum_of_accepts() {
    let port = free_udp_port();
    let mut candidate = test_guest([10, 11, 12, 0], 11, port);
    candidate.enter_failure().unwrap();
    assert!(candidate.try_start_takeover(false).await.expect("bind"));
    assert!(matches!(candidate.mode, P2pMode::SuccessorCollecting));

    // Quorum is 2 including the candidate: one accepting survivor is
    // enough, and the candidate becomes the authority.
    let ack = Takeover {
        kind: TakeoverKind::Accept,
        epoch: 2,
        turn_counter: 0,
        phase: 0,
        counters: TakeoverCounters::default(),
        node: 12,
        roster: Vec::new(),
    };
    candidate
        .on_takeover_packet(&Packet::Unknown {
            kind: HOST_TAKEOVER_PACKET_NUMBER,
            payload: ack.encode_payload(),
        })
        .await
        .unwrap();
    assert!(matches!(candidate.mode, P2pMode::Host));
    assert_eq!(candidate.accepted, vec![12]);
    if let Some(host) = candidate.host.take() {
        host.shutdown().await;
    }
}

/// Two clients with different heartbeat drift/latency eliminate the silent
/// team at **one and the same tick** (the H4 determinism gate).
#[test]
fn elimination_ticks_do_not_depend_on_heartbeat_drift() {
    // The authority decided at tick 10: team 2 was last heard at tick 6 and
    // the policy budget is 4 ticks.
    let decision = heartbeat(10, [10, 10, 6, 0], 1 << 2, [0, 0, 10, 0], 2);

    // Both clients evaluate the very same heartbeat: one receives it
    // "early", one with a delay — the recorded tick is the authority's, not
    // the local arrival time.
    let early = deterministic_eliminations(&decision, policy(4), TeamId(0));
    std::thread::sleep(Duration::from_millis(20));
    let late = deterministic_eliminations(&decision, policy(4), TeamId(0));

    assert_eq!(
        early,
        vec![TeamEliminated {
            team: TeamId(2),
            tick: 10
        }]
    );
    assert_eq!(early, late, "drift must not move the elimination tick");

    // A later heartbeat (tick 12) still reports the same elimination tick,
    // so a client that missed the tick-10 heartbeat converges to it.
    let catch_up = heartbeat(12, [12, 12, 6, 0], 1 << 2, [0, 0, 10, 0], 2);
    assert_eq!(
        deterministic_eliminations(&catch_up, policy(4), TeamId(0)),
        early,
            "a late heartbeat must not shift the tick"
    );

    // The guest view reports it exactly once with the same tick — even on
    // a client whose first observation is the later heartbeat.
    let mut view = GuestSessionView::default();
    assert_eq!(view.apply(&decision), early);
    assert!(view.apply(&catch_up).is_empty());
    let mut late_view = GuestSessionView::default();
    assert_eq!(
        late_view.apply(&catch_up),
        early,
            "a client that missed the decision heartbeat records the same tick"
    );
}

/// The elimination clock is tick-based, not wall-clock based: only
/// `advance()` moves it.
#[test]
fn session_clock_counts_numbered_ticks() {
    let roster = roster([10, 11, 12, 0], 10, 10);
    let mut clock = SessionClock::new(
        &roster,
        TeamId(0),
        policy(3),
        HOST_TIMEOUT_MIN,
        Duration::from_millis(100),
    );
    assert!(clock.advance().is_empty()); // tick 1: everyone heard at 1
    clock.heard(2);
    assert!(clock.advance().is_empty()); // tick 2
    clock.heard(2);
    assert!(clock.advance().is_empty()); // tick 3
    clock.heard(2);
    let eliminated = clock.advance(); // tick 4: team 1 silent since 1
    assert_eq!(
        eliminated,
        vec![TeamEliminated {
            team: TeamId(1),
            tick: 4
        }]
    );
    assert!(clock.eliminated[1]);
    assert_eq!(clock.eliminated_tick[1], 4);
    assert!(clock.advance().is_empty(), "eliminated once");
}

/// With 3 players (`quorum 2`) a silent guest is eliminated; with 2
/// players (`quorum 2`) the host is alone and the match pauses instead.
#[test]
fn elimination_requires_quorum() {
    let three = roster([10, 11, 12, 0], 10, 10);
    let mut clock = SessionClock::new(
        &three,
        TeamId(0),
        policy(2),
        HOST_TIMEOUT_MIN,
        Duration::from_millis(100),
    );
    clock.advance(); // tick 1
    clock.heard(1); // team 1 is alive
    clock.advance(); // tick 2
    clock.heard(1); // tick 2
                    // Tick 3: team 2 was last heard at tick 1 and the budget is 2 ticks,
                    // so its elimination tick is `1 + 2 = 3`. Team 1 is reachable, so the
                    // quorum of 2 is still met and the elimination is allowed.
    let eliminated = clock.advance();
    assert_eq!(
        eliminated,
        vec![TeamEliminated {
            team: TeamId(2),
            tick: 3
        }],
            "3 players: host + team 1 still reach the quorum 2"
    );

    let two = roster([10, 11, 0, 0], 10, 10);
    let mut clock = SessionClock::new(
        &two,
        TeamId(0),
        policy(2),
        HOST_TIMEOUT_MIN,
        Duration::from_millis(100),
    );
    for _ in 0..10 {
        assert!(
            clock.advance().is_empty(),
                "2 players: quorum 2 is not reached without the silent guest"
        );
    }
    assert!(!clock.eliminated[1]);
}

/// `Pause` (the default) never eliminates.
#[test]
fn pause_policy_never_eliminates() {
    let roster = roster([10, 11, 12, 0], 10, 10);
    let mut clock = SessionClock::new(
        &roster,
        TeamId(0),
        SessionPolicy::Pause,
        HOST_TIMEOUT_MIN,
        Duration::from_millis(100),
    );
    for _ in 0..1_000 {
        assert!(clock.advance().is_empty());
    }
    assert!(!clock.eliminated[1] && !clock.eliminated[2]);
}

/// A guest view never reports the authority's own team and validates the
/// quorum claim.
#[test]
fn guest_view_applies_eliminations_once_and_rejects_a_bad_quorum() {
    let mut view = GuestSessionView::default();
    let decision = heartbeat(10, [10, 10, 6, 0], 1 << 2, [0, 0, 10, 0], 2);
    let first = view.apply(&decision);
    assert_eq!(
        first,
        vec![TeamEliminated {
            team: TeamId(2),
            tick: 10
        }]
    );
    assert!(view.apply(&decision).is_empty(), "reported once");

    let mut stale = decision;
    stale.host_epoch = 0;
    assert!(view.apply(&stale).is_empty(), "stale epoch ignored");

    let mut bad_quorum = heartbeat(11, [11, 11, 6, 0], 1 << 2, [0, 0, 10, 0], 9);
    bad_quorum.eliminated = 1 << 2;
    assert!(
        view.apply(&bad_quorum).is_empty(),
            "a quorum above the roster is not trusted"
    );
}

/// Sanity: the module uses the plan's host-timeout range.
#[test]
fn host_timeout_bounds_are_the_plan_values() {
    assert_eq!(HOST_TIMEOUT_MIN, Duration::from_secs(5));
    assert_eq!(HOST_TIMEOUT_MAX, Duration::from_secs(300));
}

// -----------------------------------------------------------------
// Integration gates (v)/(d)/(g): real ENet, the real lockstep driver
// -----------------------------------------------------------------

use crate::net::{GameLoopStats, NetworkDispatcher, NetworkOptions, SharedDigest};
use crate::{App, AppConfig};
use max_core::enums::{PlayMode, TeamClan, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamKind, TeamUnits};
use max_core::universe::GameUniverse;

/// Session roster of the gates: host node 10 in slot 0, guests 11/12 in
/// slots 1/2.
const GATE_NODES: [u16; PLAYER_TEAM_COUNT] = [10, 11, 12, 0];
/// Heartbeat cadence of the gate authorities.
const GATE_HEARTBEAT: Duration = Duration::from_millis(50);

fn gate_hash() -> DataHash {
    DataHash::from_bytes(b"max", b"patches")
}

fn gate_session_config(port: u16) -> SessionConfig {
    SessionConfig::new("127.0.0.1", port, "", gate_hash())
}

fn gate_endpoint(port: u16) -> DirectEndpoint {
    DirectEndpoint {
        host: "127.0.0.1".to_string(),
        port,
    }
}

fn gate_reconnect() -> ReconnectPolicy {
    ReconnectPolicy {
        attempts: 6,
        timeout: MIGRATION_CONNECT_TIMEOUT,
        ..ReconnectPolicy::default()
    }
}

fn gate_options(max_turns: u32) -> NetworkOptions {
    NetworkOptions {
        max_turns,
        tick_interval: Duration::from_millis(5),
        event_wait: Duration::from_millis(2),
        host_timeout: HOST_TIMEOUT_MIN,
        ..NetworkOptions::default()
    }
}

/// A synthetic but real match: one tank per occupied team, team types of
/// `Remote_SetupPlayers` (own → `Player`, other occupied → `Remote`).
fn gate_app(local_team: TeamId) -> App<NetworkDispatcher> {
    gate_app_for(GATE_NODES, local_team)
}

fn gate_app_for(nodes: [u16; PLAYER_TEAM_COUNT], local_team: TeamId) -> App<NetworkDispatcher> {
    let mut app = App::<NetworkDispatcher>::new_network(
        AppConfig::default(),
        local_team,
        PlayMode::TurnBased,
    );
    let mut universe =
        GameUniverse::with_dispatcher(NetworkDispatcher::new(local_team, PlayMode::TurnBased));
    for team in 0..PLAYER_TEAM_COUNT {
        let team_id = TeamId(team as u8);
        universe.team_mut(team_id).expect("team slot").team_type = TeamKind::None;
    }
    *universe.team_units_table_mut() =
        std::array::from_fn(|_| TeamUnits::with_clan(TeamClan::Random));

    for (index, &node) in nodes.iter().enumerate() {
        if node == 0 {
            continue;
        }
        let team = TeamId(index as u8);
        let kind = if team == local_team {
            TeamKind::Player
        } else {
            TeamKind::Remote
        };
        universe.team_mut(team).expect("team slot").team_type = kind;
        if let Some(units) = universe.team_units_mut(team) {
            units.gold = 1_000;
        }
        universe
            .spawn_unit(
                UnitType::Tank,
                team,
                Point::new(20 + index as i16 * 5, 20),
                0,
            )
            .expect("tank spawns");
    }
    universe.set_player_team(local_team);
    universe.set_play_mode(PlayMode::TurnBased);
    app.universe = universe;
    app
}

async fn gate_connect_guest(port: u16) -> SessionIo {
    let config = gate_session_config(port);
    let session = Session::connect(config.clone()).await.expect("guest TP 00");
    SessionIo::new(session.start(), config, gate_reconnect())
}

async fn gate_run_guest(
    local_team: TeamId,
    io: P2pTransport,
    max_turns: u32,
    graceful: bool,
) -> (GameLoopStats, SharedDigest) {
    gate_run_client(GATE_NODES, local_team, io, max_turns, graceful).await
}

async fn gate_run_client(
    nodes: [u16; PLAYER_TEAM_COUNT],
    local_team: TeamId,
    io: P2pTransport,
    max_turns: u32,
    graceful: bool,
) -> (GameLoopStats, SharedDigest) {
    let mut app = gate_app_for(nodes, local_team);
    let mut io = io;
    let mut sink: Option<&mut dyn crate::net::FrameSink> = None;
    let stats = app
        .run_game_loop(&mut io, &gate_options(max_turns), &mut sink)
        .await
        .expect("guest game loop");
    if graceful {
        io.shutdown().await;
    } else {
        drop(io);
    }
    (stats, SharedDigest::capture(app.digest()))
}

/// Starts the authority and two guests on a fresh port.
async fn gate_cluster(policy: SessionPolicy) -> (P2pTransport, P2pTransport, P2pTransport) {
    let port = free_udp_port();
    let handle = HostHandle::bind(HostConfig {
        bind_ip: Ipv4Addr::LOCALHOST,
        port,
        heartbeat: Duration::ZERO,
        host_timeout: HOST_TIMEOUT_MIN,
        ..HostConfig::new(Ipv4Addr::LOCALHOST, port, gate_hash())
    })
    .expect("bind gate host");

    let g1 = gate_connect_guest(port).await;
    let g2 = gate_connect_guest(port).await;

    let host = P2pTransport::host(
        handle,
        P2pRoster::new(10, 10, GATE_NODES),
        TeamId(0),
        policy,
        HOST_TIMEOUT_MIN,
        GATE_HEARTBEAT,
    );
    let g1 = P2pTransport::guest(
        g1,
        gate_session_config(port),
        gate_endpoint(port),
        P2pRoster::new(11, 10, GATE_NODES),
        TeamId(1),
        policy,
        HOST_TIMEOUT_MIN,
        gate_reconnect(),
    );
    let g2 = P2pTransport::guest(
        g2,
        gate_session_config(port),
        gate_endpoint(port),
        P2pRoster::new(12, 10, GATE_NODES),
        TeamId(2),
        policy,
        HOST_TIMEOUT_MIN,
        gate_reconnect(),
    );
    (host, g1, g2)
}

/// H4 smoke: a two-client match with the numbered heartbeat transport
/// finishes three turns without desync (the base of the migration gates).
#[test]
fn gate_smoke_two_clients_three_turns() {
    run_gate("gate-smoke", || async {
        let port = free_udp_port();
        let nodes = [10, 11, 0, 0];
        let handle = HostHandle::bind(HostConfig {
            bind_ip: Ipv4Addr::LOCALHOST,
            port,
            heartbeat: Duration::ZERO,
            host_timeout: HOST_TIMEOUT_MIN,
            ..HostConfig::new(Ipv4Addr::LOCALHOST, port, gate_hash())
        })
        .expect("bind smoke host");
        let g1 = gate_connect_guest(port).await;

        let host = P2pTransport::host(
            handle,
            P2pRoster::new(10, 10, nodes),
            TeamId(0),
            SessionPolicy::Pause,
            HOST_TIMEOUT_MIN,
            GATE_HEARTBEAT,
        );
        let g1 = P2pTransport::guest(
            g1,
            gate_session_config(port),
            gate_endpoint(port),
            P2pRoster::new(11, 10, nodes),
            TeamId(1),
            SessionPolicy::Pause,
            HOST_TIMEOUT_MIN,
            gate_reconnect(),
        );

        let host_fut = gate_run_client(nodes, TeamId(0), host, 3, true);
        let guest_fut = gate_run_client(nodes, TeamId(1), g1, 3, true);
        let (host, guest) = tokio::join!(host_fut, guest_fut);

        assert_eq!(host.0.turns_finished, 3);
        assert_eq!(guest.0.turns_finished, 3);
        assert_eq!(host.0.desyncs, 0);
        assert_eq!(guest.0.desyncs, 0);
        assert_eq!(host.1, guest.1);
    });
}

/// Serializes the timing-sensitive integration gates: several concurrent
/// 4-worker runtimes plus the rest of the suite distort the heartbeat and
/// timeout windows these gates assert on.
static GATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// H5: a guest whose ENet link is dropped **while the authority is alive**
/// reconnects on its own — no lobby, no takeover, no new authority — and
/// publishes resume-54, so the match finishes with zero desyncs.
///
/// This closes the H4 gap where the deterministic successor (the only
/// guest of a two-player session) only ever tried a takeover: a live
/// authority was probed, but the successor never reconnected as a guest.
#[test]
fn gate_h5_guest_standalone_reconnect_resumes_with_resume54() {
    run_gate("gate-h5-reconnect", || async {
        let port = free_udp_port();
        let nodes = [10, 11, 0, 0];
        let handle = HostHandle::bind(HostConfig {
            bind_ip: Ipv4Addr::LOCALHOST,
            port,
            heartbeat: Duration::ZERO,
            host_timeout: HOST_TIMEOUT_MIN,
            ..HostConfig::new(Ipv4Addr::LOCALHOST, port, gate_hash())
        })
        .expect("bind reconnect host");
        let g1 = gate_connect_guest(port).await;

        let host = P2pTransport::host(
            handle,
            P2pRoster::new(10, 10, nodes),
            TeamId(0),
            SessionPolicy::Pause,
            HOST_TIMEOUT_MIN,
            GATE_HEARTBEAT,
        );
        let mut g1 = P2pTransport::guest(
            g1,
            gate_session_config(port),
            gate_endpoint(port),
            P2pRoster::new(11, 10, nodes),
            TeamId(1),
            SessionPolicy::Pause,
            HOST_TIMEOUT_MIN,
            gate_reconnect(),
        );
        // Guest 11 is the deterministic successor; the link break must not
        // be mistaken for a dead authority.
        let drop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        g1.set_link_drop(drop_flag.clone());

        let options = NetworkOptions {
            tick_interval: Duration::from_millis(150),
            ..gate_options(6)
        };
        let host_options = options.clone();
        let host_fut = async move {
            let mut app = gate_app_for(nodes, TeamId(0));
            let mut io = host;
            let mut sink: Option<&mut dyn crate::net::FrameSink> = None;
            let stats = app
                .run_game_loop(&mut io, &host_options, &mut sink)
                .await
                .expect("host game loop");
            io.shutdown().await;
            (stats, SharedDigest::capture(app.digest()))
        };
        let guest_fut = async move {
            let mut app = gate_app_for(nodes, TeamId(1));
            let mut io = g1;
            let mut sink: Option<&mut dyn crate::net::FrameSink> = None;
            let stats = app
                .run_game_loop(&mut io, &options, &mut sink)
                .await
                .expect("guest game loop");
            io.shutdown().await;
            (stats, SharedDigest::capture(app.digest()))
        };
        let control = async move {
            // Mid-match (the second slow turn), the guest's link is lost;
            // the ENet server of the host keeps running.
            tokio::time::sleep(Duration::from_millis(300)).await;
            drop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        };

        let (host, guest, ()) = tokio::join!(host_fut, guest_fut, control);

        assert_eq!(host.0.turns_finished, 6, "the host finished the match");
        assert_eq!(guest.0.turns_finished, 6, "the guest finished the match");
        assert_eq!(host.0.desyncs, 0, "H5 reconnect: no desyncs (host)");
        assert_eq!(guest.0.desyncs, 0, "H5 reconnect: no desyncs (guest)");
        assert_eq!(
            guest.0.reconnects, 1,
                "the guest restored its own link (resume-54 path)"
        );
        assert_eq!(host.0.migrations, 0, "the authority never changed");
        assert_eq!(guest.0.migrations, 0, "a live host must not be taken over");
        assert_eq!(host.1, guest.1, "the resumed match stays bit-identical");
    });
}

/// Runs one integration gate on a dedicated 64 MiB stack thread.
///
/// The async state machine of `run_game_loop` plus three `App` values is
/// several MiB in debug builds; the default 2 MiB test-thread stack is not
/// enough for the concurrent three-client scenarios.
fn run_gate<F, Fut>(label: &'static str, body: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()>,
{
    let handle = std::thread::Builder::new()
        .name(label.to_string())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let _guard = GATE_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()
                .expect("gate runtime");
            // A gate must fail instead of hanging the whole test binary if
            // a timing-sensitive migration step stalls under load.
            runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(90), body())
                    .await
                    .unwrap_or_else(|_| panic!("{label}: gate timed out"))
            });
            runtime.shutdown_timeout(Duration::from_secs(1));
        })
        .expect("spawn gate thread");
    if let Err(payload) = handle.join() {
        std::panic::resume_unwind(payload);
    }
}

/// Gate (v): three clients, the host is killed on turn 5; the successor
/// (`min(player_node)` = node 11) takes over and the match reaches turn 10
/// with zero desyncs and identical shared digests.
#[test]
fn gate_v_killed_host_successor_plays_to_ten_turns() {
    run_gate("gate-v", || async {
        let (host, g1, g2) = gate_cluster(SessionPolicy::Pause).await;

        let host_fut = async move {
            let mut app = gate_app(TeamId(0));
            let mut io = host;
            let mut sink: Option<&mut dyn crate::net::FrameSink> = None;
            let stats = app
                .run_game_loop(&mut io, &gate_options(5), &mut sink)
                .await
                .expect("host game loop");
            // The gate kills the process after the fifth turn, but gives
            // the last barrier (45) a moment to be flushed to both guests:
            // a takeover is only meaningful from the same lockstep point.
            tokio::time::sleep(Duration::from_millis(500)).await;
            // The host process is killed on turn 5: the transport (and
            // the ENet server) is dropped abruptly.
            drop(io);
            (stats, SharedDigest::capture(app.digest()))
        };
        let g1_fut = gate_run_guest(TeamId(1), g1, 10, true);
        let g2_fut = gate_run_guest(TeamId(2), g2, 10, true);

        let (host, guest1, guest2) = tokio::join!(host_fut, g1_fut, g2_fut);

        assert_eq!(host.0.turns_finished, 5, "the host played 5 turns");
        assert_eq!(guest1.0.turns_finished, 10, "guest 11 finished the match");
        assert_eq!(guest2.0.turns_finished, 10, "guest 12 finished the match");
        assert_eq!(guest1.0.desyncs, 0, "gate (в): no desyncs");
        assert_eq!(guest2.0.desyncs, 0, "gate (в): no desyncs");
        assert_eq!(
            guest1.0.migrations, 1,
                "guest 11 (min player_node) took over the session"
        );
        assert_eq!(
            guest2.0.migrations, 1,
                "guest 12 migrated to the new authority"
        );
        assert!(
            guest1.0.eliminations >= 1 && guest2.0.eliminations >= 1,
                "the departed host team is eliminated with quorum"
        );
        assert_eq!(
            guest1.1, guest2.1,
                "gate (в): survivors share the final state"
        );
    });
}

/// Gate (d)/3 players: a silent guest is eliminated with quorum 2 and the
/// match continues; the elimination is reported once on both survivors.
#[test]
fn gate_d_three_players_eliminate_the_silent_guest_with_quorum() {
    run_gate("gate-d", || async {
        let (host, g1, g2) = gate_cluster(SessionPolicy::EliminateAfter { ticks: 100 }).await;

        let host_fut = gate_run_guest(TeamId(0), host, 6, true);
        let g1_fut = gate_run_guest(TeamId(1), g1, 6, true);
        // Guest 12 stops after turn 3 (silent/dead), without a graceful
        // ENet disconnect; the host must notice the silence.
        let g2_fut = gate_run_guest(TeamId(2), g2, 3, false);

        let (host, guest1, _guest2) = tokio::join!(host_fut, g1_fut, g2_fut);

        assert_eq!(
            host.0.turns_finished, 6,
                "the host continues without guest 12"
        );
        assert_eq!(guest1.0.turns_finished, 6, "guest 11 continues");
        assert_eq!(host.0.eliminations, 1, "TeamEliminated on the host");
        assert_eq!(guest1.0.eliminations, 1, "TeamEliminated on guest 11");
        assert_eq!(host.0.desyncs, 0);
        assert_eq!(guest1.0.desyncs, 0);
        assert_eq!(
            host.1, guest1.1,
                "gate (d): quorum survivors share the final state"
        );
    });
}

/// Gate (d)/2 players: `quorum = 2` cannot be reached by the host alone, so
/// a lost guest freezes the match: no `TeamEliminated`, no continuation.
#[test]
fn gate_d_two_players_pause_without_elimination() {
    run_gate("gate-d2", || async {
        let port = free_udp_port();
        let nodes = [10, 11, 0, 0];
        let handle = HostHandle::bind(HostConfig {
            bind_ip: Ipv4Addr::LOCALHOST,
            port,
            heartbeat: Duration::ZERO,
            host_timeout: HOST_TIMEOUT_MIN,
            ..HostConfig::new(Ipv4Addr::LOCALHOST, port, gate_hash())
        })
        .expect("bind 2p host");
        let g1 = gate_connect_guest(port).await;

        let policy = SessionPolicy::EliminateAfter { ticks: 2 };
        let host = P2pTransport::host(
            handle,
            P2pRoster::new(10, 10, nodes),
            TeamId(0),
            policy,
            HOST_TIMEOUT_MIN,
            GATE_HEARTBEAT,
        );
        let g1 = P2pTransport::guest(
            g1,
            gate_session_config(port),
            gate_endpoint(port),
            P2pRoster::new(11, 10, nodes),
            TeamId(1),
            policy,
            HOST_TIMEOUT_MIN,
            gate_reconnect(),
        );

        let guest_fut = gate_run_client(nodes, TeamId(1), g1, 2, false);
        let host_fut = gate_run_client(nodes, TeamId(0), host, 4, true);
        // The guest leaves after two turns; the host must stay at the
        // barrier instead of eliminating it and continuing.
        let host_outcome = tokio::time::timeout(Duration::from_secs(3), host_fut);
        let (host_outcome, guest) = tokio::join!(host_outcome, guest_fut);

        assert_eq!(guest.0.turns_finished, 2, "the guest played two turns");
        assert!(
            host_outcome.is_err(),
                "2 players (quorum 2): the host pauses instead of continuing"
        );
    });
}

/// Gate (g): a relay break (split-brain injection) freezes both sides with
/// no divergence; after the merge the heartbeat resumes the match.
#[test]
fn gate_g_split_brain_pauses_both_sides_and_resumes_after_merge() {
    run_gate("gate-g", || async {
        let (mut host, mut g1, mut g2) = gate_cluster(SessionPolicy::Pause).await;
        // The relay is broken on all three sides; the host ENet server is
        // still alive (a relay break, not a dead process).
        let blackout = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag_host = blackout.clone();
        let flag_g1 = blackout.clone();
        let flag_g2 = blackout.clone();
        let flag_control = blackout.clone();
        host.set_blackout(flag_host);
        g1.set_blackout(flag_g1);
        g2.set_blackout(flag_g2);
        // Slow the turn pacing down so the break lands mid-match (after a
        // completed turn), not between the initial and the first turn.
        let options = NetworkOptions {
            tick_interval: Duration::from_millis(150),
            ..gate_options(6)
        };

        let host_options = options.clone();
        let host_fut = async move {
            let mut app = gate_app(TeamId(0));
            let mut io = host;
            let mut sink: Option<&mut dyn crate::net::FrameSink> = None;
            let stats = app
                .run_game_loop(&mut io, &host_options, &mut sink)
                .await
                .expect("host game loop");
            io.shutdown().await;
            (stats, SharedDigest::capture(app.digest()))
        };
        let g1_options = options.clone();
        let g1_fut = async move {
            let mut app = gate_app(TeamId(1));
            let mut io = g1;
            let mut sink: Option<&mut dyn crate::net::FrameSink> = None;
            let stats = app
                .run_game_loop(&mut io, &g1_options, &mut sink)
                .await
                .expect("guest 11 game loop");
            io.shutdown().await;
            (stats, SharedDigest::capture(app.digest()))
        };
        let g2_options = options;
        let g2_fut = async move {
            let mut app = gate_app(TeamId(2));
            let mut io = g2;
            let mut sink: Option<&mut dyn crate::net::FrameSink> = None;
            let stats = app
                .run_game_loop(&mut io, &g2_options, &mut sink)
                .await
                .expect("guest 12 game loop");
            io.shutdown().await;
            (stats, SharedDigest::capture(app.digest()))
        };
        // 200 ms of play (one slow turn), then a 5.5 s relay break
        // (longer than host_timeout), then a heal: both sides are frozen in
        // between and no successor may appear.
        let control = async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            flag_control.store(true, std::sync::atomic::Ordering::Relaxed);
            tokio::time::sleep(HOST_TIMEOUT_MIN + Duration::from_millis(500)).await;
            flag_control.store(false, std::sync::atomic::Ordering::Relaxed);
        };

        let (host, guest1, guest2, ()) = tokio::join!(host_fut, g1_fut, g2_fut, control);

        assert_eq!(host.0.turns_finished, 6, "the host resumed after the merge");
        assert_eq!(guest1.0.turns_finished, 6, "guest 11 resumed");
        assert_eq!(guest2.0.turns_finished, 6, "guest 12 resumed");
        assert_eq!(host.0.desyncs, 0, "gate (г): no divergence");
        assert_eq!(guest1.0.desyncs, 0);
        assert_eq!(guest2.0.desyncs, 0);
        assert_eq!(host.1, guest1.1, "gate (г): host and guest 11 agree");
        assert_eq!(guest1.1, guest2.1, "gate (г): survivors agree");
        assert_eq!(
            guest1.0.migrations, 0,
                "a healed relay must not create a second authority"
        );
        assert_eq!(host.0.migrations, 0, "the old host stays the authority");
    });
}
