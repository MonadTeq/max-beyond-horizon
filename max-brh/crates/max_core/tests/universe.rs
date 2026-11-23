//! Stage A facade tests — `max_core::universe` (ТЗ `game-universe-stage-a.md` §3).
//!
//! What is proved here:
//!
//! 1. **Equivalence (key test):** a `submit`→`tick` script produces a
//!    byte-identical `snapshot_bytes` to an `enqueue`→`tick` run of the same
//!    script (the reference script is `GameState::scripted_replay_is_deterministic`,
//!    `game.rs:3458`);
//! 2. `seq`/`delta_since` semantics;
//! 3. `EventStream`: one buffer, independent cursors, events accumulate per tick
//!    and are drained on request;
//! 4. `submit_remote(StateSync)` applies immediately and bumps `seq`;
//!    `submit_remote(Command)` only queues;
//! 5. `LocalDispatcher`: `route` is always `Local`, `plan` is empty,
//!    `drain_outgoing` is empty, the barrier counters stay untouched;
//! 6. `SubmitOutcome`: a dropped route returns `Dropped` and reports the full
//!    `CommandRejected` context, an accepted one returns `Accepted`;
//! 7. **Gate 4** (owner item 4): a single-player cycle
//!    `submit(06) → tick → finish_turn → turn_counter + 1` passes **without**
//!    `step_*_barrier` (the counters stay `0`) and its `snapshot_bytes` is
//!    identical to the network-style run with the barriers disabled.

use max_core::crc;
use max_core::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};
use max_core::enums::{GameStateType, PlayMode, TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::events::{
    command_end_turn_barrier, command_finished_turn, command_move_linear, payload_p1_for,
};
use max_core::geo::Point;
use max_core::input::{
    Command, OrderPayload, Packet13Seed, Packet50StopUnit, PayloadP2, PayloadP3,
};
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::unit::UnitId;
use max_core::universe::{
    BarrierFacts, Dispatcher, EventStream, GameEvent, GameUniverse, LocalDispatcher,
    NetworkCommand, RejectSource, RemoteCommand, RemoteHandling, RoutedCommand, SubmitOutcome,
};
use max_core::GameState;

// ---------------------------------------------------------------------------
// Scenario helpers
// ---------------------------------------------------------------------------

/// Two-team turn-based game with one tank per side — the `scripted_replay` setup.
fn base_state() -> (GameState, UnitId, UnitId) {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = TeamKind::Computer;
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units[1] = TeamUnits::with_clan(TeamClan::Random);

    let red = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("red tank");
    let green = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(20, 10),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("green tank");

    (state, red, green)
}

/// One frame of the script: commands submitted together, then `ticks` steps.
struct Frame {
    commands: Vec<Command>,
    ticks: usize,
}

/// The `scripted_replay_is_deterministic` scenario, expressed as frames.
fn replay_frames(red: UnitId, green: UnitId) -> Vec<Frame> {
    vec![
        Frame {
            commands: vec![
                Command::Seed(Packet13Seed { seed: 0xDEAD_BEEF }),
                Command::Order(max_core::input::Packet08Order {
                    unit: red,
                    payload: OrderPayload::P3(PayloadP3 {
                        p2: PayloadP2 {
                            p1: payload_p1_for(
                                UnitOrder::Move,
                                UnitOrderState::Init,
                                UnitOrder::Await,
                                UnitOrderState::ExecutingOrder,
                            ),
                            parent_unit: None,
                            experience: 0,
                        },
                        move_to: Point::new(14, 10),
                        fire_on: Point::new(14, 10),
                        enemy_unit: Some(green),
                        transfer_cargo: 0,
                        stealth_dice_roll: 0,
                    }),
                }),
            ],
            ticks: 6,
        },
        Frame {
            commands: vec![command_move_linear(
                green,
                Point::new(20, 10),
                Point::new(20, 13),
                UnitOrder::Move,
                UnitOrderState::InProgress,
            )],
            ticks: 1,
        },
        Frame {
            commands: vec![
                command_move_linear(
                    red,
                    Point::new(14, 10),
                    Point::new(18, 10),
                    UnitOrder::Move,
                    UnitOrderState::InProgress,
                ),
                Command::Stop(Packet50StopUnit { unit: green }),
                Command::MoveFinished(max_core::input::Packet41MoveFinished::port_form(red, true)),
            ],
            ticks: 5,
        },
        Frame {
            commands: vec![
                command_finished_turn(TeamId::RED),
                command_finished_turn(TeamId::GREEN),
                command_end_turn_barrier(TeamId::RED, 1),
            ],
            ticks: 1,
        },
    ]
}

/// Which command path drives the script.
#[derive(Clone, Copy)]
enum Path {
    /// The old path: `GameState::enqueue` + `GameState::tick`.
    Legacy,
    /// The facade path: `GameUniverse::submit` + `GameUniverse::tick`.
    Facade,
}

/// Runs the script and returns the snapshot after every simulated tick.
fn run_script(path: Path) -> Vec<Vec<u8>> {
    let (state, red, green) = base_state();
    let frames = replay_frames(red, green);
    let mut universe = GameUniverse::new(state, LocalDispatcher);
    let mut snapshots = Vec::new();

    for frame in frames {
        match path {
            Path::Legacy => {
                for command in frame.commands {
                    universe.state_mut().enqueue(command);
                }
                for _ in 0..frame.ticks {
                    universe.state_mut().tick();
                    snapshots.push(universe.state().snapshot_bytes());
                }
            }
            Path::Facade => {
                for command in frame.commands {
                    assert_eq!(universe.submit(command), SubmitOutcome::Accepted);
                }
                for _ in 0..frame.ticks {
                    universe.tick();
                    snapshots.push(universe.state().snapshot_bytes());
                }
            }
        }
    }

    // The turn close, as the scripted reference does it.
    let _ = universe.state_mut().finish_turn();
    for _ in 0..3 {
        universe.tick();
        snapshots.push(universe.state().snapshot_bytes());
    }

    snapshots
}

/// A duel state where a tank fires on its neighbour: a source of `Sound` events.
fn duel_state() -> GameState {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = TeamKind::Computer;
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units[1] = TeamUnits::with_clan(TeamClan::Random);
    state.active_turn_team = TeamId::RED;
    state.player_team = TeamId::RED;

    let attacker = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("attacker");
    let target = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(11, 10),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("target");
    let _ = target;

    let mut data = state.order_data(attacker);
    data.fire_on = Point::new(11, 10);
    data.move_to = Point::new(11, 10);
    state.set_order_data(attacker, data);
    state.set_order(attacker, UnitOrder::Fire, UnitOrderState::Init);

    state
}

// ---------------------------------------------------------------------------
// 1. Equivalence — the key test
// ---------------------------------------------------------------------------

#[test]
fn submit_tick_matches_enqueue_tick_byte_for_byte() {
    let legacy = run_script(Path::Legacy);
    let facade = run_script(Path::Facade);

    assert_eq!(
        legacy.len(),
        facade.len(),
        "the runs must advance the same number of ticks"
    );
    assert!(!legacy.is_empty());

    for (index, (old, new)) in legacy.iter().zip(facade.iter()).enumerate() {
        assert_eq!(
            old, new,
            "snapshot_bytes diverged after tick {index}: submit→tick ≠ enqueue→tick"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. seq / delta_since
// ---------------------------------------------------------------------------

#[test]
fn delta_since_reports_the_current_version_once() {
    let (state, _red, _green) = base_state();
    let mut universe = GameUniverse::new(state, LocalDispatcher);

    assert_eq!(universe.seq(), 0);
    assert!(
        universe.delta_since(0).is_none(),
        "a fresh universe has no changes to report"
    );

    universe.tick();

    assert_eq!(universe.seq(), 1);
    let delta = universe.delta_since(0).expect("one tick is a change");
    assert_eq!(delta.from_seq, 0);
    assert_eq!(delta.seq, 1);
    assert_eq!(delta.checksum, crc::state_digest(universe.state()));
    assert_eq!(delta.checksum, delta.state.state_digest());
    assert_eq!(
        delta.state.snapshot_bytes(),
        universe.state().snapshot_bytes()
    );
    assert!(
        universe.delta_since(1).is_none(),
        "the caller is already up to date"
    );
}

// ---------------------------------------------------------------------------
// 3. EventStream — one buffer, independent cursors
// ---------------------------------------------------------------------------

#[test]
fn event_streams_have_independent_cursors() {
    let mut universe = GameUniverse::new(duel_state(), LocalDispatcher);
    let mut render = universe.subscribe();
    let mut qa = universe.subscribe();

    // Combat sounds accumulate during the ticks; nobody drains yet.
    for _ in 0..12 {
        universe.tick();
    }

    let render_events = render.drain(&mut universe);
    let qa_events = qa.drain(&mut universe);

    assert!(
        render_events
            .iter()
            .any(|event| matches!(event, GameEvent::Sound(_))),
        "the duel must produce combat sounds: {render_events:?}"
    );
    assert_eq!(
        render_events, qa_events,
        "both streams start at the same cursor and must see the same events"
    );
    assert!(
        render.drain(&mut universe).is_empty(),
        "the cursor has been advanced: no events twice"
    );
    assert!(qa.drain(&mut universe).is_empty());
}

#[test]
fn events_accumulate_per_tick_and_are_drained_on_request() {
    let mut universe = GameUniverse::new(duel_state(), LocalDispatcher);
    let mut stream = universe.subscribe();

    // A stream subscribed later must not see the earlier events.
    for _ in 0..6 {
        universe.tick();
    }
    let mut late = universe.subscribe();

    for _ in 0..6 {
        universe.tick();
    }

    let early = stream.drain(&mut universe);
    let late_events = late.drain(&mut universe);

    assert!(!late_events.is_empty(), "the second half produced sounds");
    assert!(
        early.len() > late_events.len(),
        "the early stream must see the first half too: {} vs {}",
        early.len(),
        late_events.len()
    );
}

// ---------------------------------------------------------------------------
// 4. submit_remote
// ---------------------------------------------------------------------------

/// A state with a remote team and one of its units — a packet-54 receiver.
fn remote_state() -> (GameState, UnitId) {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = TeamKind::Remote;
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units[1] = TeamUnits::with_clan(TeamClan::Random);

    let green = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(20, 10),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("green tank");

    (state, green)
}

/// A stand-in for the stage-B `NetworkDispatcher`: simulation commands are
/// broadcast, barrier commands stay local, peers are replayed in receive order and
/// packet 54 is applied as authoritative state.
struct TestNetworkDispatcher;

impl Dispatcher for TestNetworkDispatcher {
    fn route(&mut self, cmd: Command) -> RoutedCommand {
        match cmd {
            Command::TurnBarrier(_)
            | Command::EndTurnBarrier(_)
            | Command::CrcBarrier(_)
            | Command::FrameBarrier(_) => RoutedCommand::Local(cmd),
            other => RoutedCommand::Broadcast(other),
        }
    }

    fn receive(&mut self, cmd: RemoteCommand) -> RemoteHandling {
        match cmd {
            RemoteCommand::Command(command) => RemoteHandling::Replay(command),
            RemoteCommand::StateSync(packet) => RemoteHandling::ApplyStateSync(packet),
        }
    }

    fn plan(&mut self, _facts: &BarrierFacts) -> Vec<NetworkCommand> {
        Vec::new()
    }
}

#[test]
fn submit_remote_applies_state_sync_immediately_and_queues_commands() {
    let (state, green) = remote_state();
    let mut universe = GameUniverse::new(state, TestNetworkDispatcher);

    let hits_before = universe.state().unit(green).expect("unit").hits;
    let packet = max_core::events::StateSyncPacket {
        team: TeamId::GREEN,
        sync_index: 1,
        is_last: false,
        units: vec![(
            green,
            max_core::events::UnitStatePacket {
                unit_type: UnitType::Tank as u16,
                team: TeamId::GREEN.0,
                hits: hits_before.saturating_sub(1),
                orders: UnitOrder::Sentry as u8,
                state: UnitOrderState::ExecutingOrder as u8,
                prior_orders: UnitOrder::Sentry as u8,
                prior_state: UnitOrderState::ExecutingOrder as u8,
                ..Default::default()
            },
        )],
        tail: None,
    };

    universe.submit_remote(RemoteCommand::StateSync(packet));

    assert_eq!(
        universe.state().unit(green).expect("unit").hits,
        hits_before.saturating_sub(1),
        "the authoritative state is applied without a tick"
    );
    assert_eq!(universe.seq(), 1, "a state sync is a state version");

    // A command is only queued.
    universe.submit_remote(RemoteCommand::Command(command_finished_turn(TeamId::GREEN)));
    assert!(
        !universe.state().teams[1].finished_turn,
        "a remote command must not be applied before the tick"
    );
    assert_eq!(universe.seq(), 1);

    universe.tick();
    assert!(
        universe.state().teams[1].finished_turn,
        "the queued command is applied at the start of the next tick"
    );
    assert_eq!(universe.seq(), 2);
}

// ---------------------------------------------------------------------------
// 5. LocalDispatcher
// ---------------------------------------------------------------------------

#[test]
fn local_dispatcher_is_pure_and_does_not_step_barriers() {
    let mut dispatcher = LocalDispatcher;
    let mut universe = GameUniverse::new(duel_state(), LocalDispatcher);

    let command = command_finished_turn(TeamId::RED);
    assert_eq!(
        dispatcher.route(command.clone()),
        RoutedCommand::Local(command),
        "a local match routes every command as Local"
    );

    let facts = BarrierFacts {
        begin_turn: Some(0),
        end_turn: Some(0),
        crc: Some(0),
        state_sync: [0; max_core::TEAM_SLOTS],
        local_team: TeamId::RED,
        play_mode: PlayMode::TurnBased,
    };
    assert!(
        dispatcher.plan(&facts).is_empty(),
        "a local match has no barrier plan"
    );

    for _ in 0..8 {
        universe.tick();
    }

    assert!(
        universe.drain_outgoing().is_empty(),
        "a local match produces no transport DTOs"
    );
    assert!(
        universe.drain_outgoing().is_empty(),
        "the outgoing buffer is drained once"
    );

    // Gate-4 part: in single player the barrier counters are never stepped.
    let state = universe.state();
    assert_eq!(state.frame_sync_counter, [0; max_core::TEAM_SLOTS]);
    assert_eq!(state.turn_indices, [0; max_core::TEAM_SLOTS]);
    assert_eq!(state.next_turn_indices, [0; max_core::TEAM_SLOTS]);
    assert_eq!(state.team_data_crc16, [0; max_core::TEAM_SLOTS]);
    assert_eq!(state.begin_turn_barrier(), Some(0));
    assert_eq!(state.end_turn_barrier(), Some(0));
    assert_eq!(state.crc_barrier(), Some(0));

    // `LocalDispatcher::receive` is `Ignore`: a single-player universe never
    // takes peer input (ADR §2).
    universe.submit_remote(RemoteCommand::Command(command_finished_turn(TeamId::GREEN)));
    assert!(
        universe.state().commands.is_empty(),
        "a local dispatcher ignores remote commands"
    );
}

// ---------------------------------------------------------------------------
// 6. SubmitOutcome / CommandRejected
// ---------------------------------------------------------------------------

/// A dispatcher that drops `50` and keeps everything else local.
struct DroppingDispatcher;

impl Dispatcher for DroppingDispatcher {
    fn route(&mut self, cmd: Command) -> RoutedCommand {
        if matches!(cmd, Command::Stop(_)) {
            RoutedCommand::Drop
        } else {
            RoutedCommand::Local(cmd)
        }
    }

    fn receive(&mut self, _cmd: RemoteCommand) -> RemoteHandling {
        RemoteHandling::Ignore
    }

    fn plan(&mut self, _facts: &BarrierFacts) -> Vec<NetworkCommand> {
        Vec::new()
    }
}

#[test]
fn submit_reports_dropped_and_accepted_with_full_context() {
    let (state, _red, green) = base_state();
    let mut universe = GameUniverse::new(state, DroppingDispatcher);

    let forbidden = Command::Stop(Packet50StopUnit { unit: green });
    assert_eq!(universe.submit(forbidden.clone()), SubmitOutcome::Dropped);
    assert!(
        universe.state().commands.is_empty(),
        "a dropped command never reaches the queue"
    );

    let mut stream = universe.subscribe();
    // The rejection was pushed before the subscription: subscribe at 0 instead.
    let mut stream_from_start = EventStream::new();
    let events = stream_from_start.drain(&mut universe);
    let rejection = events
        .iter()
        .find_map(|event| match event {
            GameEvent::CommandRejected {
                cmd,
                outcome,
                tick,
                source,
            } => Some((cmd.clone(), *outcome, *tick, *source)),
            _ => None,
        })
        .expect("the rejection is reported");
    assert_eq!(rejection.0, forbidden);
    assert_eq!(rejection.1, max_core::events::ApplyOutcome::Ignored);
    assert_eq!(rejection.2, 0);
    assert_eq!(rejection.3, RejectSource::DroppedByRoute);
    assert!(stream.drain(&mut universe).is_empty());

    assert_eq!(
        universe.submit(command_finished_turn(TeamId::RED)),
        SubmitOutcome::Accepted
    );
    assert_eq!(universe.state().commands.len(), 1);
}

// ---------------------------------------------------------------------------
// 7. Gate 4 (owner item 4) — single-player cycle without barrier steppers
// ---------------------------------------------------------------------------

/// A turn-based two-team state at the end of the RED turn.
fn gate4_state() -> GameState {
    let (mut state, _red, _green) = base_state();
    state.play_mode = PlayMode::TurnBased;
    state.game_state = GameStateType::InGame;
    state.active_turn_team = TeamId::RED;
    state.player_team = TeamId::RED;
    state
}

#[test]
fn single_player_turn_cycle_closes_without_barrier_steppers() {
    // Single player: the facade, `LocalDispatcher`, no barrier steppers.
    let mut universe = GameUniverse::new(gate4_state(), LocalDispatcher);
    let turn_before = universe.state().turn_counter;

    assert_eq!(
        universe.submit(command_finished_turn(TeamId::RED)),
        SubmitOutcome::Accepted
    );
    universe.tick();
    assert_eq!(
        universe.state().game_state,
        GameStateType::EndTurn,
        "packet 06 put the turn-based match into the end-of-turn phase"
    );
    assert!(universe.state().teams[0].finished_turn);

    // The core turn close (`finish_turn`) is called by the turn machine; in
    // stage A next_team is not inside tick yet, so the test drives it explicitly.
    let over = universe.state_mut().finish_turn();
    assert!(!over, "the match is not over");
    universe.tick();

    assert_eq!(
        universe.state().turn_counter,
        turn_before + 1,
        "the turn counter advanced exactly once"
    );

    // No barrier stepper was called: every barrier counter is still zero.
    let single = universe.state();
    assert_eq!(single.frame_sync_counter, [0; max_core::TEAM_SLOTS]);
    assert_eq!(single.turn_indices, [0; max_core::TEAM_SLOTS]);
    assert_eq!(single.next_turn_indices, [0; max_core::TEAM_SLOTS]);
    assert_eq!(single.team_data_crc16, [0; max_core::TEAM_SLOTS]);

    // The network-style driver with the barriers disabled: the same commands
    // through the queue (00/52/45 are **not** sent, the steppers are not called).
    let mut network = gate4_state();
    network.enqueue(command_finished_turn(TeamId::RED));
    network.tick();
    network.finish_turn();
    network.tick();

    assert_eq!(
        single.snapshot_bytes(),
        network.snapshot_bytes(),
        "single player and the barrier-free network run must be byte-identical"
    );
    assert_eq!(single.turn_counter, network.turn_counter);

    // Positive control: the barrier steppers do change the CRC-relevant counters,
    // so the equality above is not vacuous.
    let mut with_barriers = gate4_state();
    let _ = with_barriers.step_begin_turn_barrier(TeamId::RED);
    let _ = with_barriers.step_end_turn_barrier(TeamId::RED);
    let _ = with_barriers.step_crc_barrier(TeamId::RED);
    assert_ne!(
        with_barriers.snapshot_bytes(),
        single.snapshot_bytes(),
        "stepped barriers must be visible in the snapshot"
    );
}
