use super::*;

use max_core::enums::{TeamClan, UnitType};
use max_core::geo::Point;
use max_core::input::{
    Packet01FrameBarrier, Packet06FinishedTurn, Packet10UpgradeUnitType, Packet13Seed,
    Packet18Chat, Packet45CrcBarrier, Packet53ComplexSummary,
};
use max_core::team::TeamUnits;
use max_core::universe::{LocalDispatcher, RemoteCommand, SubmitOutcome};
use max_core::UnitId;
use max_net::complex_summary_for;

/// Synthetic world: the local team is `Player`, `remotes` are `Remote`, the rest
/// are `None` (`Remote_SetupPlayers` team types).
fn synthetic_universe(local: TeamId, remotes: &[TeamId]) -> GameUniverse<LocalDispatcher> {
    let mut universe = GameUniverse::default();
    for team in 0..PLAYER_TEAM_COUNT {
        universe
            .team_mut(TeamId(team as u8))
            .expect("team slot")
            .team_type = TeamKind::None;
    }
    *universe.team_units_table_mut() =
        std::array::from_fn(|_| TeamUnits::with_clan(TeamClan::Random));
    universe.team_mut(local).expect("local slot").team_type = TeamKind::Player;
    for team in remotes {
        universe.team_mut(*team).expect("remote slot").team_type = TeamKind::Remote;
    }
    universe.set_player_team(local);
    universe
}

/// The synthetic team setup on top of an empty world with an arbitrary dispatcher.
fn synthetic_universe_with<D: Dispatcher>(
    local: TeamId,
    remotes: &[TeamId],
    dispatcher: D,
) -> GameUniverse<D> {
    let mut universe = GameUniverse::with_dispatcher(dispatcher);
    for team in 0..PLAYER_TEAM_COUNT {
        universe
            .team_mut(TeamId(team as u8))
            .expect("team slot")
            .team_type = TeamKind::None;
    }
    *universe.team_units_table_mut() =
        std::array::from_fn(|_| TeamUnits::with_clan(TeamClan::Random));
    universe.team_mut(local).expect("local slot").team_type = TeamKind::Player;
    for team in remotes {
        universe.team_mut(*team).expect("remote slot").team_type = TeamKind::Remote;
    }
    universe.set_player_team(local);
    universe
}

fn barrier_counter(packet: &Packet) -> u8 {
    match packet {
        Packet::Command(Command::TurnBarrier(barrier)) => barrier.counter,
        Packet::Command(Command::EndTurnBarrier(barrier)) => barrier.counter,
        Packet::Command(Command::CrcBarrier(barrier)) => barrier.next_turn_index,
        other => panic!("not a counter barrier: {other:?}"),
    }
}

// Stage C A1: the driver's two-step barrier calls — the facade counter step
// followed by the FSM record. These are exactly the two lines
// `App<NetworkDispatcher>` runs, so the tests keep covering the production path.

fn begin<D: Dispatcher>(
    fsm: &mut NetworkDispatcher,
    universe: &mut GameUniverse<D>,
) -> Vec<Packet> {
    let counter = universe.step_begin_turn_barrier(fsm.local_team());
    fsm.record_begin(counter)
}

fn complex<D: Dispatcher>(
    fsm: &mut NetworkDispatcher,
    universe: &GameUniverse<D>,
) -> Vec<Packet> {
    let turn = universe.snapshot().turn_counter() + 1;
    fsm.record_complex(turn);
    vec![complex_summary_for(
        universe.snapshot(),
        fsm.local_team(),
        turn,
    )]
}

fn end<D: Dispatcher>(
    fsm: &mut NetworkDispatcher,
    universe: &mut GameUniverse<D>,
) -> Vec<Packet> {
    let counter = universe.step_end_turn_barrier(fsm.local_team());
    fsm.record_end(counter)
}

fn sync<D: Dispatcher>(fsm: &mut NetworkDispatcher, universe: &GameUniverse<D>) -> Vec<Packet> {
    let index = fsm.record_sync();
    state_sync_for(universe.snapshot(), fsm.local_team(), index)
        .into_iter()
        .map(Packet::StateSync)
        .collect()
}

fn crc<D: Dispatcher>(
    fsm: &mut NetworkDispatcher,
    universe: &mut GameUniverse<D>,
) -> Vec<Packet> {
    let index = universe.step_crc_barrier(fsm.local_team());
    let checksum = universe.snapshot().state_checksum();
    fsm.record_crc(index, checksum)
}

#[test]
fn counters_start_at_one_and_are_pre_incremented_per_team() {
    let local = TeamId::RED;
    let mut universe = synthetic_universe(local, &[TeamId::GREEN, TeamId::BLUE]);
    let mut fsm = NetworkDispatcher::new(local, PlayMode::TurnBased);

    let begin = begin(&mut fsm, &mut universe);
    assert_eq!(begin.len(), 1);
    assert_eq!(barrier_counter(&begin[0]), 1, "the first 00 is 1");
    assert_eq!(
        universe.snapshot().frame_sync_counter()[local.0 as usize],
        1
    );
    // The own counter is not yet agreed with the peers.
    assert!(!fsm.begin_reached(&universe));
    universe.set_frame_sync_counter(TeamId::GREEN, 1);
    assert!(!fsm.begin_reached(&universe), "all peers are required");
    universe.set_frame_sync_counter(TeamId::BLUE, 1);
    assert!(fsm.begin_reached(&universe));

    let end = end(&mut fsm, &mut universe);
    assert_eq!(barrier_counter(&end[0]), 1, "the first 52 is 1");
    assert_eq!(universe.snapshot().turn_indices()[local.0 as usize], 1);
    assert!(!fsm.end_reached(&universe));
    universe.set_turn_index(TeamId::GREEN, 1);
    universe.set_turn_index(TeamId::BLUE, 1);
    assert!(fsm.end_reached(&universe));

    let sync = sync(&mut fsm, &universe);
    assert!(!sync.is_empty());
    match &sync[0] {
        Packet::StateSync(packet) => {
            assert_eq!(packet.sync_index, 1, "the first 54 is 1");
            assert_eq!(packet.team, local);
            assert!(packet.is_last, "an empty team is one last chunk");
        }
        other => panic!("expected 54, got {other:?}"),
    }
    assert!(!fsm.sync_reached(&universe));
    universe.set_state_sync_index(TeamId::GREEN, 1);
    universe.set_state_sync_index(TeamId::BLUE, 1);
    assert!(fsm.sync_reached(&universe));

    let crc = crc(&mut fsm, &mut universe);
    assert_eq!(barrier_counter(&crc[0]), 1, "the first 45 is 1");
    assert_eq!(universe.snapshot().next_turn_indices()[local.0 as usize], 1);
    let own_crc = match &crc[0] {
        Packet::Command(Command::CrcBarrier(barrier)) => barrier.crc16,
        other => panic!("expected 45, got {other:?}"),
    };
    assert_eq!(own_crc, universe.snapshot().state_checksum());
    assert!(!fsm.crc_reached(&universe));
    universe.set_next_turn_index(TeamId::GREEN, 1);
    universe.set_next_turn_index(TeamId::BLUE, 1);
    assert!(fsm.crc_reached(&universe));
    assert!(
        !fsm.crc_matches(&universe),
            "the peer CRCs are not here yet"
    );
    universe.set_team_data_crc16(TeamId::GREEN, own_crc);
    universe.set_team_data_crc16(TeamId::BLUE, own_crc);
    assert!(fsm.crc_matches(&universe));
}

#[test]
fn fsm_walks_the_full_turn_sequence() {
    let local = TeamId::RED;
    let mut universe = synthetic_universe(local, &[TeamId::GREEN]);
    let mut fsm = NetworkDispatcher::new(local, PlayMode::TurnBased);

    // 00
    let begin = begin(&mut fsm, &mut universe);
    assert_eq!(fsm.phase(), BarrierPhase::Begin);
    assert_eq!(barrier_counter(&begin[0]), 1);
    universe.set_frame_sync_counter(TeamId::GREEN, 1);
    assert!(fsm.reached(&universe));

    // 53
    let play = complex(&mut fsm, &universe);
    assert_eq!(fsm.phase(), BarrierPhase::Play);
    assert!(matches!(
        &play[0],
        Packet::Command(Command::ComplexSummary(summary)) if summary.turn == 1
    ));
    fsm.note_complex(TeamId::GREEN, 1);
    assert!(fsm.reached(&universe));

    // 06 — local end of the turn (exactly once).
    fsm.mark_local_finished();
    assert!(fsm.local_finished_sent());

    // 52
    let end = end(&mut fsm, &mut universe);
    assert_eq!(fsm.phase(), BarrierPhase::End);
    assert_eq!(barrier_counter(&end[0]), 1);
    universe.set_turn_index(TeamId::GREEN, 1);
    assert!(fsm.reached(&universe));

    // 54
    let sync = sync(&mut fsm, &universe);
    assert_eq!(fsm.phase(), BarrierPhase::Sync);
    assert!(matches!(&sync[0], Packet::StateSync(packet) if packet.sync_index == 1));
    universe.set_state_sync_index(TeamId::GREEN, 1);
    assert!(fsm.reached(&universe));

    // 45
    let crc = crc(&mut fsm, &mut universe);
    assert_eq!(fsm.phase(), BarrierPhase::Crc);
    assert_eq!(barrier_counter(&crc[0]), 1);
    universe.set_next_turn_index(TeamId::GREEN, 1);
    universe.set_team_data_crc16(TeamId::GREEN, fsm.crc_checksum());
    assert!(fsm.reached(&universe));
    assert!(fsm.crc_matches(&universe));

    // finish_turn → the next turn starts with a clean 06.
    fsm.on_turn_finished();
    assert!(!fsm.local_finished_sent());
}

#[test]
fn same_kind_barriers_use_separate_counters() {
    let local = TeamId::RED;
    let mut universe = synthetic_universe(local, &[TeamId::GREEN]);
    let mut fsm = NetworkDispatcher::new(local, PlayMode::TurnBased);

    begin(&mut fsm, &mut universe);
    end(&mut fsm, &mut universe);
    sync(&mut fsm, &universe);
    crc(&mut fsm, &mut universe);
    assert_eq!(fsm.begin_counter(), 1);
    assert_eq!(fsm.end_counter(), 1);
    assert_eq!(fsm.sync_index(), 1);
    assert_eq!(fsm.crc_index(), 1);

    // Next turn: each counter has its own pre-increment (2).
    fsm.on_turn_finished();
    let begin = begin(&mut fsm, &mut universe);
    assert_eq!(barrier_counter(&begin[0]), 2);
    let end = end(&mut fsm, &mut universe);
    assert_eq!(barrier_counter(&end[0]), 2);
    let sync = sync(&mut fsm, &universe);
    assert!(matches!(
        &sync[0],
        Packet::StateSync(packet) if packet.sync_index == 2
    ));
    let crc = crc(&mut fsm, &mut universe);
    assert_eq!(barrier_counter(&crc[0]), 2);

    // 52 and 45 are different C++ arrays, not one shared counter.
    let state = universe.snapshot();
    assert_eq!(state.turn_indices()[local.0 as usize], 2);
    assert_eq!(state.next_turn_indices()[local.0 as usize], 2);
    assert_eq!(state.frame_sync_counter()[local.0 as usize], 2);
}

#[test]
fn complex_barrier_needs_53_from_every_peer_for_the_current_turn() {
    let local = TeamId::RED;
    let mut universe = synthetic_universe(local, &[TeamId::GREEN, TeamId::BLUE]);
    // Rust turn_counter = 0 → C++ GameManager_TurnCounter = 1.
    let mut fsm = NetworkDispatcher::new(local, PlayMode::TurnBased);
    let packet = complex(&mut fsm, &universe);
    assert!(matches!(
        &packet[0],
        Packet::Command(Command::ComplexSummary(summary)) if summary.turn == 1
    ));
    assert!(!fsm.reached(&universe));

    fsm.note_complex(TeamId::GREEN, 1);
    assert!(!fsm.reached(&universe), "BLUE has not sent 53 yet");
    fsm.note_complex(TeamId::BLUE, 1);
    assert!(fsm.reached(&universe));

    // The 53 of the previous turn does not close the new one.
    universe.set_turn_counter(1);
    let packet = complex(&mut fsm, &universe);
    assert!(matches!(
        &packet[0],
        Packet::Command(Command::ComplexSummary(summary)) if summary.turn == 2
    ));
    assert!(!fsm.reached(&universe));
    fsm.note_complex(TeamId::GREEN, 2);
    fsm.note_complex(TeamId::BLUE, 2);
    assert!(fsm.reached(&universe));
}

#[test]
fn single_player_barriers_pass_immediately() {
    let local = TeamId::RED;
    let mut universe = synthetic_universe(local, &[]);
    let mut fsm = NetworkDispatcher::new(local, PlayMode::TurnBased);

    let packets = begin(&mut fsm, &mut universe);
    assert!(!packets.is_empty());
    assert!(fsm.reached(&universe), "00 with no peers");

    let packets = complex(&mut fsm, &universe);
    assert!(!packets.is_empty());
    assert!(fsm.reached(&universe), "53 with no peers");

    let packets = end(&mut fsm, &mut universe);
    assert!(!packets.is_empty());
    assert!(fsm.reached(&universe), "52 with no peers");

    let packets = sync(&mut fsm, &universe);
    assert!(!packets.is_empty());
    assert!(fsm.reached(&universe), "54 with no peers");

    let packets = crc(&mut fsm, &mut universe);
    assert!(!packets.is_empty());
    assert!(fsm.reached(&universe), "45 with no peers");
    assert!(fsm.crc_matches(&universe));
}

#[test]
fn turn_based_runs_begin_once_simultaneous_every_turn() {
    let local = TeamId::RED;

    assert!(!NetworkDispatcher::new(local, PlayMode::TurnBased).simultaneous());
    assert!(NetworkDispatcher::new(local, PlayMode::SimultaneousMoves).simultaneous());
}

#[test]
fn local_06_is_sent_once_per_turn_and_reset_on_finish() {
    let local = TeamId::RED;
    let mut fsm = NetworkDispatcher::new(local, PlayMode::TurnBased);

    assert!(!fsm.local_finished_sent());
    fsm.mark_local_finished();
    assert!(fsm.local_finished_sent());

    // A new turn resets the flag: 06 will be sent again.
    fsm.on_turn_finished();
    assert!(!fsm.local_finished_sent(), "a new turn — a new 06");
}

#[test]
fn barrier_timings_are_the_cpp_constants() {
    assert_eq!(
        BARRIER_RETRY,
        Duration::from_secs(3),
            "REMOTE_PING_TIME_PERIOD must stay 3 s"
    );
    assert_eq!(
        BARRIER_TIMEOUT,
        Duration::from_secs(30),
            "REMOTE_RESPONSE_TIMEOUT must stay 30 s"
    );
    assert_eq!(BARRIER_RETRY, DEFAULT_BARRIER_REPEAT);
    assert_eq!(BARRIER_TIMEOUT, DEFAULT_BARRIER_TIMEOUT);
}

#[test]
fn barrier_pending_repeats_every_three_seconds_and_reports_timeout_once() {
    let start = Instant::now();
    let packet = Packet::Command(Command::CrcBarrier(Packet45CrcBarrier {
        team: TeamId::RED,
        next_turn_index: 1,
        crc16: 0,
    }));
    let mut pending = BarrierPending::new(vec![packet], start);

    assert!(pending.poll(start).is_none());
    assert!(matches!(
        pending.poll(start + Duration::from_secs(3)),
        Some(BarrierPoll::Retransmit)
    ));
    assert_eq!(pending.attempts, 1);
    assert!(matches!(
        pending.poll(start + Duration::from_secs(6)),
        Some(BarrierPoll::Retransmit)
    ));
    assert_eq!(pending.attempts, 2);

    // The timeout is logged once, but the record does not disappear — the barrier
    // keeps repeating, like a C++ ping.
    assert!(matches!(
        pending.poll(start + Duration::from_secs(30)),
        Some(BarrierPoll::TimedOut)
    ));
    assert!(matches!(
        pending.poll(start + Duration::from_secs(31)),
        Some(BarrierPoll::Retransmit)
    ));
    assert!(pending.poll(start + Duration::from_secs(32)).is_none());
}

/// The same three-client world as the `net.rs` disconnect scenario: one tank per
/// team, clients only differ in the team kinds.
fn three_team_state() -> (GameUniverse<LocalDispatcher>, [UnitId; 3]) {
    let mut state = GameUniverse::default();
    let teams = [TeamId::RED, TeamId::GREEN, TeamId::BLUE];
    let mut ids: [UnitId; 3] = [UnitId::NO_ID; 3];

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

/// Resume-54 after a reconnect: own team, the last sync_index and the full
/// set of team units (including the economy tail in the last chunk).
#[test]
fn resume_state_sync_publishes_the_last_completed_index() {
    let (universe, ids) = three_team_state();
    let mut fsm = NetworkDispatcher::new(TeamId::GREEN, PlayMode::SimultaneousMoves);
    fsm.set_sync_index(7);

    let packets = fsm.resume_state_sync(&universe);
    assert_eq!(packets.len(), 1, "one unit — one last chunk");
    match &packets[0] {
        Packet::StateSync(sync) => {
            assert_eq!(sync.team, TeamId::GREEN);
            assert_eq!(sync.sync_index, 7, "the last completed round");
            assert!(sync.is_last);
            assert_eq!(sync.units.len(), 1);
            assert_eq!(sync.units[0].0, ids[1], "only the own team's units");
        }
        other => panic!("expected 54, got {other:?}"),
    }
}

// -----------------------------------------------------------------------
// Dispatcher policy (ADR §2)
// -----------------------------------------------------------------------

fn barrier_facts() -> BarrierFacts {
    BarrierFacts {
        begin_turn: Some(0),
        end_turn: Some(0),
        crc: Some(0),
        state_sync: [0; max_core::TEAM_SLOTS],
        local_team: TeamId::RED,
        play_mode: PlayMode::TurnBased,
    }
}

#[test]
fn dispatcher_routes_simulation_broadcast_barriers_local_and_seed_drop() {
    let mut dispatcher = NetworkDispatcher::new(TeamId::RED, PlayMode::TurnBased);

    let simulation = Command::FinishedTurn(Packet06FinishedTurn { team: TeamId::RED });
    assert_eq!(
        dispatcher.route(simulation.clone()),
        RoutedCommand::Broadcast(simulation)
    );
    let move_order = Command::UpgradeUnitType(Packet10UpgradeUnitType {
        team: TeamId::RED,
        unit_type: UnitType::Tank,
        values: [0; 13],
    });
    assert_eq!(
        dispatcher.route(move_order.clone()),
        RoutedCommand::Broadcast(move_order)
    );
    let chat = Command::Chat(Packet18Chat {
        sender: TeamId::RED,
        message: "hi".into(),
    });
    assert_eq!(
        dispatcher.route(chat.clone()),
        RoutedCommand::Broadcast(chat),
            "18 is a broadcast command (ADR §2)"
    );

    // Packet 09 must travel the broadcast channel: the team economy is part of
    // the simulated state (gold and research allocation enter the CRC), so a
    // `Local` route would desync the owner from the peers.
    let team_state = Command::TeamState(max_core::input::Packet09TeamState {
        team: TeamId::RED,
        gold: 500,
        gold_spent_on_upgrades: 40,
        research: [max_core::team::ResearchTopicInfo::default();
            max_core::enums::RESEARCH_TOPIC_COUNT],
        name: "RED".into(),
    });
    assert_eq!(
        dispatcher.route(team_state.clone()),
        RoutedCommand::Broadcast(team_state),
            "09 is a broadcast command (remote.cpp:2085 REMOTE_MULTICAST)"
    );

    for barrier in [
        Command::TurnBarrier(Packet00TurnBarrier {
            team: TeamId::RED,
            counter: 1,
        }),
        Command::EndTurnBarrier(Packet52EndTurnBarrier {
            team: TeamId::RED,
            counter: 1,
        }),
        Command::CrcBarrier(Packet45CrcBarrier {
            team: TeamId::RED,
            next_turn_index: 1,
            crc16: 0,
        }),
        Command::ComplexSummary(Packet53ComplexSummary {
            team: TeamId::RED,
            turn: 1,
            complexes: Vec::new(),
        }),
    ] {
        assert_eq!(
            dispatcher.route(barrier.clone()),
            RoutedCommand::Local(barrier),
                "a barrier stays local"
        );
    }

    assert_eq!(
        dispatcher.route(Command::Seed(Packet13Seed { seed: 7 })),
        RoutedCommand::Drop,
            "a mid-match 13 is forbidden"
    );
}

#[test]
fn dispatcher_receive_replays_commands_and_applies_state_sync() {
    let mut dispatcher = NetworkDispatcher::new(TeamId::RED, PlayMode::TurnBased);

    let command = Command::FinishedTurn(Packet06FinishedTurn {
        team: TeamId::GREEN,
    });
    assert_eq!(
        dispatcher.receive(RemoteCommand::Command(command.clone())),
        RemoteHandling::Replay(command)
    );

    let packet = max_core::events::StateSyncPacket {
        team: TeamId::GREEN,
        sync_index: 2,
        is_last: false,
        units: Vec::new(),
        tail: None,
    };
    assert_eq!(
        dispatcher.receive(RemoteCommand::StateSync(packet.clone())),
        RemoteHandling::ApplyStateSync(packet)
    );
}

/// A4: the consent barriers `00`/`52`/`45` are applied immediately, not replayed.
#[test]
fn dispatcher_receive_applies_consent_barriers_immediately() {
    let mut dispatcher = NetworkDispatcher::new(TeamId::RED, PlayMode::TurnBased);
    let team = TeamId::GREEN;

    for barrier in [
        Command::TurnBarrier(max_core::input::Packet00TurnBarrier { team, counter: 3 }),
        Command::EndTurnBarrier(max_core::input::Packet52EndTurnBarrier { team, counter: 4 }),
        Command::CrcBarrier(max_core::input::Packet45CrcBarrier {
            team,
            next_turn_index: 5,
            crc16: 0x1234,
        }),
    ] {
        assert_eq!(
            dispatcher.receive(RemoteCommand::Command(barrier.clone())),
            RemoteHandling::ApplyBarrier(barrier),
                "a consent barrier is immediate (A4)"
        );
    }

    // `01` is a pacing packet, not a consent barrier: it keeps the replay path.
    let frame = Command::FrameBarrier(Packet01FrameBarrier { team, counter: 1 });
    assert_eq!(
        dispatcher.receive(RemoteCommand::Command(frame.clone())),
        RemoteHandling::Replay(frame)
    );
}

#[test]
fn dispatcher_never_advances_turns_and_has_no_barrier_plan() {
    let mut dispatcher = NetworkDispatcher::new(TeamId::RED, PlayMode::TurnBased);
    assert!(!dispatcher.advances_turns());
    assert!(dispatcher.plan(&barrier_facts()).is_empty());
}

#[test]
fn facade_universe_with_network_dispatcher_broadcasts_and_drops() {
    let local = TeamId::RED;
    let mut universe = synthetic_universe_with(
        local,
        &[TeamId::GREEN],
        NetworkDispatcher::new(local, PlayMode::TurnBased),
    );

    let simulation = Command::FinishedTurn(Packet06FinishedTurn { team: local });
    assert_eq!(universe.submit(simulation.clone()), SubmitOutcome::Accepted);
    assert_eq!(
        universe.drain_outgoing(),
        vec![NetworkCommand::Command(simulation)]
    );

    // A barrier submitted locally is not broadcast.
    let barrier = Command::TurnBarrier(Packet00TurnBarrier {
        team: local,
        counter: 1,
    });
    assert_eq!(universe.submit(barrier), SubmitOutcome::Accepted);
    assert!(universe.drain_outgoing().is_empty());

    assert_eq!(
        universe.submit(Command::Seed(Packet13Seed { seed: 7 })),
        SubmitOutcome::Dropped
    );
    assert!(
        universe.drain_outgoing().is_empty(),
            "a dropped command never reaches the transport"
    );
}
