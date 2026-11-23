//! Phase 1 integration: command/event dispatch (`events.rs`).
//!
//! Oracle: `max-cpp-port/src/remote.cpp` (`Remote_ProcessNetPackets` and the
//! `Remote_ReceiveNetPacket_*` handlers), `unitevents.cpp`
//! (`UnitEventEmergencyStop::Process`) and `max-brh-docs/protocol.md` §8.
//!
//! Scenario shape: commands enqueued in receive order → one tick (delivery at
//! the start of the loop) → effects on units/barrers → the same batch twice is
//! bit-identical (D). Consumers are checked through their observable state
//! (unit order/path, event queue, counters).

use max_core::crc;
use max_core::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};
use max_core::enums::{PlayMode, TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::events::{self, ApplyOutcome};
use max_core::game::GameState;
use max_core::geo::Point;
use max_core::input::{
    Command, OrderPayload, Packet00TurnBarrier, Packet08Order, Packet13Seed, Packet18Chat,
    Packet50StopUnit,
};
use max_core::team::{TeamId, TeamKind, TeamUnits};

const TANK_FLAGS: u32 = MOBILE_LAND_UNIT | SELECTABLE;

/// A local two-human game with one tank per side.
fn dispatcher_state() -> GameState {
    let mut state = GameState::new();
    state.network_game = false;
    state.play_mode = PlayMode::TurnBased;

    for team in [TeamId::RED, TeamId::GREEN] {
        let index = team.0 as usize;
        state.teams[index].team_type = TeamKind::Player;
        state.team_units[index] = TeamUnits::with_clan(TeamClan::Random);
    }

    state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), TANK_FLAGS)
        .expect("red tank");
    state
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(20, 10),
            TANK_FLAGS,
        )
        .expect("green tank");

    state
}

fn tank_of(state: &GameState, team: TeamId) -> max_core::UnitId {
    state
        .unit_ids_in_order()
        .into_iter()
        .find(|id| state.unit(*id).is_some_and(|unit| unit.team == team))
        .expect("tank")
}

fn order_command(unit: max_core::UnitId, order: UnitOrder) -> Command {
    Command::Order(Packet08Order {
        unit,
        payload: OrderPayload::P1(events::payload_p1_for(
            order,
            UnitOrderState::ExecutingOrder,
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder,
        )),
    })
}

/// A batch of commands is applied in receive order: the last order of a unit
/// wins, chat and seed reach their consumers, and the count of applied
/// commands equals the batch size.
#[test]
fn batch_is_applied_in_receive_order() {
    let mut state = dispatcher_state();
    let red = tank_of(&state, TeamId::RED);

    state.enqueue(order_command(red, UnitOrder::Sentry));
    state.enqueue(Command::Chat(Packet18Chat {
        sender: TeamId::GREEN,
        message: "ready".into(),
    }));
    state.enqueue(order_command(red, UnitOrder::Await));
    state.enqueue(Command::Seed(Packet13Seed { seed: 0x0BAD_F00D }));

    assert_eq!(state.commands.len(), 4);
    assert_eq!(state.apply_pending_commands(), 4);
    assert!(state.commands.is_empty());
    assert_eq!(state.commands.applied(), 4);

    assert_eq!(state.unit(red).expect("red tank").orders, UnitOrder::Await);
    assert_eq!(state.chat_log.len(), 1);
    assert_eq!(state.chat_log[0].0, TeamId::GREEN);
    assert_eq!(state.chat_log[0].1, "ready");
    assert_eq!(state.rng, max_core::Randomizer::new(0x0BAD_F00D));

    // The reverse order of the two order commands yields the other final order.
    let mut reversed = dispatcher_state();
    reversed.enqueue(order_command(red, UnitOrder::Await));
    reversed.enqueue(order_command(red, UnitOrder::Sentry));
    reversed.apply_pending_commands();
    assert_eq!(
        reversed.unit(red).expect("red tank").orders,
        UnitOrder::Sentry
    );
    assert_ne!(reversed.snapshot_bytes(), state.snapshot_bytes());
}

/// Packet 50 does not move the unit itself: it queues a `UnitEventEmergencyStop`
/// which the order cycle consumes on the next tick (event → consumer).
#[test]
fn stop_event_is_delivered_to_the_order_cycle() {
    fn run() -> GameState {
        let mut state = dispatcher_state();
        let red = tank_of(&state, TeamId::RED);

        state.enqueue(events::command_move_linear(
            red,
            Point::new(10, 10),
            Point::new(14, 10),
            UnitOrder::Move,
            UnitOrderState::InProgress,
        ));
        state.tick();
        assert!(
            state.unit(red).expect("red tank").path.is_some(),
            "the move order installed a path"
        );

        state.enqueue(Command::Stop(Packet50StopUnit { unit: red }));
        // Delivery only records the event; the cycle consumes it in the same tick.
        state.tick();

        assert!(state.unit_events.is_empty(), "the event queue drains");
        assert!(
            state.unit(red).expect("red tank").path.is_none(),
            "the stop event reached the unit"
        );

        state
    }

    let first = run();
    let second = run();

    assert_eq!(first.snapshot_bytes(), second.snapshot_bytes());
    assert_eq!(crc::state_digest(&first), crc::state_digest(&second));
    assert_eq!(
        crc::replay_digest_bytes(&first),
        crc::replay_digest_bytes(&second)
    );
}

/// A rejected command in the middle of a batch does not block the following
/// ones: the dispatcher reports `UnknownUnit` and keeps draining the queue.
#[test]
fn unknown_unit_does_not_block_the_rest_of_the_batch() {
    let mut state = dispatcher_state();
    let red = tank_of(&state, TeamId::RED);
    let missing = max_core::UnitId(0x7FFE);

    let unknown = order_command(missing, UnitOrder::Sentry);
    assert_eq!(
        events::apply_command(&mut state, &unknown),
        ApplyOutcome::UnknownUnit
    );

    state.enqueue(unknown);
    state.enqueue(Command::Chat(Packet18Chat {
        sender: TeamId::RED,
        message: "still here".into(),
    }));
    state.enqueue(Command::Seed(Packet13Seed { seed: 7 }));

    assert_eq!(state.apply_pending_commands(), 3);
    assert_eq!(state.chat_log.len(), 1);
    assert_eq!(state.rng, max_core::Randomizer::new(7));
    assert_eq!(state.unit(red).expect("red tank").orders, UnitOrder::Await);
}

/// Barrier delivery is ordered and idempotent: a repeated packet keeps the same
/// counter and the state stays bit-identical; diverging counters never assemble
/// a barrier (D).
#[test]
fn barrier_delivery_is_ordered_and_idempotent() {
    let mut base = dispatcher_state();
    base.network_game = true;
    // Only RED plays, so a single report assembles the barrier.
    base.teams[TeamId::GREEN.0 as usize].team_type = TeamKind::None;

    let mut once = base.clone();
    once.enqueue(Command::TurnBarrier(Packet00TurnBarrier {
        team: TeamId::RED,
        counter: 3,
    }));
    assert_eq!(once.apply_pending_commands(), 1);
    assert_eq!(once.begin_turn_barrier(), Some(3));

    let mut twice = once.clone();
    twice.enqueue(Command::TurnBarrier(Packet00TurnBarrier {
        team: TeamId::RED,
        counter: 3,
    }));
    assert_eq!(twice.apply_pending_commands(), 1);
    assert_eq!(twice.begin_turn_barrier(), Some(3));
    assert_eq!(
        twice.snapshot_bytes(),
        once.snapshot_bytes(),
        "repeating the same barrier packet is idempotent"
    );

    // Two reports with diverging counters never assemble a barrier.
    let mut diverged = dispatcher_state();
    diverged.network_game = true;
    diverged.enqueue(Command::TurnBarrier(Packet00TurnBarrier {
        team: TeamId::RED,
        counter: 1,
    }));
    diverged.enqueue(Command::TurnBarrier(Packet00TurnBarrier {
        team: TeamId::GREEN,
        counter: 2,
    }));
    assert_eq!(diverged.apply_pending_commands(), 2);
    assert_eq!(diverged.begin_turn_barrier(), None);
}
