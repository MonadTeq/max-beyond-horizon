//! SDL/UI Stage B, B3 — the `apply_now` vs `submit`+`tick` equivalence gate.
//!
//! The application has two ways to hand a command to the simulation:
//!
//! * the **immediate** channel — `GameUniverse::apply_now` (through
//!   `App::flush_orders` for the demo HUD queue, the build menu, the build site
//!   and chat): the effect must be observable before the next frame;
//! * the **lockstep** channel — `GameUniverse::submit` (the network driver and the
//!   local one): the command is queued and applied by the next `tick`.
//!
//! The tick applies the queue at its step 2 (`GameState::tick`), **before**
//! `process_orders`, so both channels must produce byte-identical simulation state
//! after the same number of ticks. A difference is not an accepted "tick shift":
//! it means the command was classified into the wrong channel (a barrier-consent
//! packet that must be applied immediately, or a command that must be queued) —
//! the stage-B gate stops there.
//!
//! `06` is the one deliberate exception (owner decision 1): the turn close goes
//! through `GameUniverse::end_local_turn` (immediate `06` + the team-turn machine),
//! never through `submit(06)` — the queued form enters `EndTurn` again inside the
//! next tick. [`end_local_turn_is_not_the_queued_06`] keeps the two
//! paths distinguishable, as the stage-A gate required.
//!
//! Stage D (`sdl-ui-integration.md` §3: the safety net before C3) extends the gate:
//!
//! * the command table must cover **every** UI command kind — `09`, `08/P1–P5`,
//!   `38`, `41`, `50`, `18`, `10`, `11` — not a sample
//!   ([`the_gate_covers_every_ui_command_kind`]);
//! * packet `11` (the `AllocMenu` complex totals) is in the table and is proven to
//!   reach the complex through both channels
//!   ([`packet_11_totals_reach_the_complex_through_both_channels`]);
//! * the local team-turn machine is pinned as deterministic: the same call
//!   sequence gives the same bytes, digests, `(turn_counter, active team)` trace
//!   and `Turn` notices, both against a computer and in hot-seat
//!   ([`the_local_turn_machine_is_deterministic`]).

use std::collections::BTreeSet;

use max_app::{App, SessionChannel};
use max_app::AppConfig;
use max_core::enums::{ResearchTopic, TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::events::{command_move_linear, command_order_p1, payload_p1_for, ApplyOutcome};
use max_core::game::TurnEvent;
use max_core::geo::Point;
use max_core::input::{
    Command, OrderPayload, Packet08Order, Packet09TeamState, Packet10UpgradeUnitType,
    Packet11ComplexOrder, Packet18Chat, Packet41MoveFinished, Packet50StopUnit, PacketKind,
    PayloadP2, PayloadP3, PayloadP4, PayloadP5,
};
use max_core::research::upgrade_packet_values;
use max_core::team::{ResearchTopicInfo, TeamId, TeamKind, TeamUnits};
use max_core::unit::UnitId;
use max_core::universe::{GameEvent, GameUniverse, LocalDispatcher, TurnNotice};

/// How many ticks each variant runs after the command (`N + 1` of the gate).
const TICKS: u32 = 4;

/// A deterministic duel: RED tank at (10,10), GREEN tank at (14,10).
///
/// Two calls produce byte-identical universes — the gate compares them, so the
/// fixture itself is checked by [`fixtures_are_byte_identical`].
fn duel() -> (GameUniverse<LocalDispatcher>, UnitId, UnitId) {
    let mut universe = GameUniverse::default();
    universe.set_player_team(TeamId::RED);
    universe.set_active_turn_team(TeamId::RED);
    universe.set_play_mode(max_core::enums::PlayMode::TurnBased);
    for (slot, kind) in [
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::None,
        TeamKind::None,
    ]
    .into_iter()
    .enumerate()
    {
        universe.teams_mut()[slot].team_type = kind;
    }
    for slot in 0..2 {
        *universe
            .team_units_table_mut()
            .get_mut(slot)
            .expect("team economy") = TeamUnits::with_clan(TeamClan::Random);
    }

    let red = universe
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
        .expect("RED tank");
    let green = universe
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(14, 10), 0)
        .expect("GREEN tank");

    // Packet 11 is the *totals* order of an existing complex (`AllocMenu` →
    // `Complex::WritePacket`), so the fixture needs one: an empty lowest-id
    // complex of RED. `create_complex` is a pure id/order operation, so the
    // fixture stays byte-identical across calls (see `fixtures_are_byte_identical`).
    let complex = universe
        .team_units_mut(TeamId::RED)
        .expect("RED economy")
        .create_complex();
    assert_eq!(complex.0, 1, "the fixture complex is the lowest free id");

    (universe, red, green)
}

/// Every command kind the UI used to apply through `apply_now`.
///
/// The list mirrors the call sites of the demo path: the HUD order queue
/// (`38`/`08` P1–P5, `41`, `50`, `18`), the build menu/site (`08` P4), the
/// upgrade screen (`10`), the menu commits (`09`) and the `AllocMenu` complex
/// totals (`11`). A new command kind that reaches `hud.pending` must be added
/// here: the gate is exhaustive for the UI paths, not a sample —
/// [`the_gate_covers_every_ui_command_kind`] fails when one disappears.
fn ui_commands(
    universe: &GameUniverse<LocalDispatcher>,
    red: UnitId,
    green: UnitId,
) -> Vec<(&'static str, Command)> {
    // The payload level must match the order (`OrderPayload::matches_order`):
    // P1 — Sentry, P2 — Repair, P3 — MoveToUnit, P4 — Build, P5 — NewAllocate.
    let p1 = payload_p1_for(
        UnitOrder::Sentry,
        UnitOrderState::ExecutingOrder,
        UnitOrder::Await,
        UnitOrderState::ExecutingOrder,
    );
    let p2 = PayloadP2 {
        p1: payload_p1_for(
            UnitOrder::Repair,
            UnitOrderState::Init,
            UnitOrder::Await,
            UnitOrderState::ExecutingOrder,
        ),
        parent_unit: Some(green),
        experience: 0,
    };
    let p3 = PayloadP3 {
        p2: PayloadP2 {
            p1: payload_p1_for(
                UnitOrder::MoveToUnit,
                UnitOrderState::Init,
                UnitOrder::Await,
                UnitOrderState::ExecutingOrder,
            ),
            parent_unit: Some(green),
            experience: 0,
        },
        move_to: Point::new(14, 10),
        fire_on: Point::new(14, 10),
        enemy_unit: Some(green),
        transfer_cargo: 3,
        stealth_dice_roll: 7,
    };
    let p4 = PayloadP4 {
        p3: PayloadP3 {
            p2: PayloadP2 {
                p1: payload_p1_for(
                    UnitOrder::Build,
                    UnitOrderState::Init,
                    UnitOrder::Await,
                    UnitOrderState::ExecutingOrder,
                ),
                parent_unit: None,
                experience: 0,
            },
            move_to: Point::new(10, 10),
            fire_on: Point::ZERO,
            enemy_unit: None,
            transfer_cargo: 0,
            stealth_dice_roll: 0,
        },
        repeat_build: false,
        build_time: 1,
        build_rate: 1,
        build_list: vec![UnitType::Infantry],
    };
    let p5 = PayloadP5 {
        p3: PayloadP3 {
            p2: PayloadP2 {
                p1: payload_p1_for(
                    UnitOrder::NewAllocate,
                    UnitOrderState::Init,
                    UnitOrder::PowerOn,
                    UnitOrderState::ExecutingOrder,
                ),
                parent_unit: None,
                experience: 0,
            },
            move_to: Point::new(10, 10),
            fire_on: Point::ZERO,
            enemy_unit: None,
            transfer_cargo: 0,
            stealth_dice_roll: 0,
        },
        total_mining: 9,
        raw_mining: 4,
        fuel_mining: 3,
        gold_mining: 2,
    };
    let upgrade_values = upgrade_packet_values(
        &universe.snapshot().team_units()[TeamId::RED.0 as usize].current_values
            [UnitType::Tank as usize],
    );

    vec![
        ("08/P1 sentry", command_order_p1(red, p1)),
        (
            "08/P2 repair",
            Command::Order(Packet08Order {
                unit: red,
                payload: OrderPayload::P2(p2),
            }),
        ),
        (
            "08/P3 move-to-unit",
            Command::Order(Packet08Order {
                unit: red,
                payload: OrderPayload::P3(p3),
            }),
        ),
        (
            "08/P4 build",
            Command::Order(Packet08Order {
                unit: red,
                payload: OrderPayload::P4(p4),
            }),
        ),
        (
            "08/P5 new-allocate",
            Command::Order(Packet08Order {
                unit: red,
                payload: OrderPayload::P5(p5),
            }),
        ),
        (
            "38 move",
            command_move_linear(
                red,
                Point::new(10, 10),
                Point::new(11, 10),
                UnitOrder::Move,
                UnitOrderState::InProgress,
            ),
        ),
        (
            "41 move-finished",
            Command::MoveFinished(Packet41MoveFinished::port_form(red, false)),
        ),
        ("50 stop", Command::Stop(Packet50StopUnit { unit: red })),
        (
            "18 chat",
            Command::Chat(Packet18Chat {
                sender: TeamId::RED,
                message: "equivalence".to_string(),
            }),
        ),
        (
            // `AllocMenu` «Run»: the six totals of the selected complex
            // (`allocmenu.cpp:790-792` → `Complex::WritePacket` → 11).
            "11 complex order",
            Command::ComplexOrder(Packet11ComplexOrder {
                team: TeamId::RED,
                complex_id: 1,
                raw: 42,
                fuel: 43,
                gold: 44,
                power: 45,
                workers: 46,
                buildings: 47,
            }),
        ),
        (
            "10 upgrade",
            Command::UpgradeUnitType(Packet10UpgradeUnitType {
                team: TeamId::RED,
                unit_type: UnitType::Tank,
                values: upgrade_values,
            }),
        ),
        (
            "09 team state (gold)",
            Command::TeamState(Packet09TeamState {
                team: TeamId::RED,
                gold: 400,
                gold_spent_on_upgrades: 12,
                research: universe.snapshot().teams()[TeamId::RED.0 as usize].research_topics,
                name: "RED".to_string(),
            }),
        ),
        (
            "09 team state (research)",
            Command::TeamState(Packet09TeamState {
                team: TeamId::RED,
                gold: 400,
                gold_spent_on_upgrades: 12,
                research: {
                    let mut topics =
                        universe.snapshot().teams()[TeamId::RED.0 as usize].research_topics;
                    topics[ResearchTopic::Shots as usize] = ResearchTopicInfo {
                        research_level: 0,
                        turns_to_complete: 16,
                        allocation: 1,
                    };
                    topics
                },
                name: "RED".to_string(),
            }),
        ),
    ]
}

#[test]
fn fixtures_are_byte_identical() {
    let (first, _, _) = duel();
    let (second, _, _) = duel();

    assert_eq!(
        first.snapshot().snapshot_bytes(),
        second.snapshot().snapshot_bytes(),
        "the gate fixture must be deterministic, otherwise the comparison is meaningless"
    );
}

/// The gate is exhaustive for the UI command kinds, not a sample.
///
/// `ui_commands` is the single table both equivalence tests walk; if a kind is
/// dropped from it (or a new one reaches `hud.pending`/`submit` without being
/// added), the byte-equivalence proof silently stops covering it. The expected
/// set is the Stage D list: `09`, `08/P1–P5`, `38`, `41`, `50`, `18`, `10`, `11`.
/// Barriers (`00`/`01`/`45`/`52`) and `06` are outside it by decision — they are
/// consent/FSM packets, not UI intents (`06` goes through `end_local_turn`).
#[test]
fn the_gate_covers_every_ui_command_kind() {
    let (fixture, red, green) = duel();
    let commands = ui_commands(&fixture, red, green);

    let covered: BTreeSet<PacketKind> = commands
        .iter()
        .map(|(_, command)| command.kind())
        .collect();

    let expected: BTreeSet<PacketKind> = [
        PacketKind::OrderUnit,      // 08/P1–P5
        PacketKind::MoveWithPath,   // 38
        PacketKind::MoveFinished,   // 41
        PacketKind::StopUnit,       // 50
        PacketKind::Chat,           // 18
        PacketKind::ComplexOrder,   // 11
        PacketKind::TeamState,      // 09
        PacketKind::UpgradeUnitType, // 10
    ]
    .into_iter()
    .collect();

    assert_eq!(
        covered, expected,
        "the equivalence gate must cover every UI command kind"
    );
}

/// Packet 11 is not a vacuous gate entry: the complex totals must really reach
/// the state through **both** channels (an `Applied` outcome alone would only
/// prove that the packet was routed, not that the comparison had content).
#[test]
fn packet_11_totals_reach_the_complex_through_both_channels() {
    let totals = (42i16, 43i16, 44i16, 45i16, 46i16, 47i16);
    let (fixture, red, green) = duel();
    let commands = ui_commands(&fixture, red, green);
    let (name, command) = commands
        .into_iter()
        .find(|(name, _)| *name == "11 complex order")
        .expect("the gate lists packet 11");

    // Immediate channel: visible before the first tick.
    let (mut immediate, _, _) = duel();
    assert_eq!(
        immediate.apply_now(command.clone()),
        ApplyOutcome::Applied,
        "{name}: the immediate channel applies the totals order"
    );
    let complex = immediate
        .snapshot()
        .complex(TeamId::RED, 1)
        .expect("RED complex");
    assert_eq!(
        (
            complex.raw,
            complex.fuel,
            complex.gold,
            complex.power,
            complex.workers,
            complex.buildings
        ),
        totals,
        "{name}: the immediate channel writes every complex field"
    );

    // Lockstep channel: the same totals are applied by the next tick.
    let (mut queued, _, _) = duel();
    assert_eq!(
        queued.submit(command),
        max_core::universe::SubmitOutcome::Accepted
    );
    queued.tick();
    let complex = queued.snapshot().complex(TeamId::RED, 1).expect("RED complex");
    assert_eq!(
        (
            complex.raw,
            complex.fuel,
            complex.gold,
            complex.power,
            complex.workers,
            complex.buildings
        ),
        totals,
        "{name}: the lockstep channel writes every complex field"
    );
}

/// The local team-turn machine (`begin_local_round`/`end_local_turn`) is a pure
/// deterministic function of the state and the call sequence.
///
/// The gate drives two identical fixtures through two full rounds (RED and GREEN
/// close their turns) and compares the canonical bytes, the digest and the
/// `GameEvent::Turn` notice stream after every step. Both team configurations are
/// pinned: RED (human) vs GREEN (computer) and the hot-seat pair (two humans, the
/// «Begin turn» notice path). A difference means the machine picked up a
/// non-deterministic input (wall clock, iteration order, RNG) — the "turn machine
/// is deterministic" precondition of Stage D.
#[test]
fn the_local_turn_machine_is_deterministic() {
    fn record(
        universe: &GameUniverse<LocalDispatcher>,
        bytes: &mut Vec<Vec<u8>>,
        digests: &mut Vec<u16>,
        trace: &mut Vec<(i32, u8)>,
    ) {
        let snapshot = universe.snapshot();
        bytes.push(snapshot.snapshot_bytes());
        digests.push(snapshot.state_digest());
        trace.push((snapshot.turn_counter(), snapshot.active_turn_team().0));
    }

    fn run(hot_seat: bool) -> (Vec<Vec<u8>>, Vec<u16>, Vec<(i32, u8)>, Vec<GameEvent>) {
        let (mut universe, _, _) = duel();
        if hot_seat {
            universe.teams_mut()[TeamId::GREEN.0 as usize].team_type = TeamKind::Player;
        }
        let mut events = universe.subscribe();
        let mut bytes = Vec::new();
        let mut digests = Vec::new();
        let mut trace = Vec::new();

        universe.begin_local_round();
        record(&universe, &mut bytes, &mut digests, &mut trace);

        // Two teams × two rounds: each call closes the turn of the acting team
        // and advances the machine to the next one (`RoundClosed` on every second).
        for _ in 0..4 {
            let acting = universe.snapshot().active_turn_team();
            universe.end_local_turn(acting);
            record(&universe, &mut bytes, &mut digests, &mut trace);
        }

        (bytes, digests, trace, events.drain(&mut universe))
    }

    for hot_seat in [false, true] {
        let label = if hot_seat { "hot-seat" } else { "vs computer" };

        let (first_bytes, first_digests, first_trace, first_events) = run(hot_seat);
        let (second_bytes, second_digests, second_trace, second_events) = run(hot_seat);

        assert_eq!(
            first_bytes, second_bytes,
            "{label}: the turn machine must produce byte-identical states on a repeated run"
        );
        assert_eq!(
            first_digests, second_digests,
            "{label}: the turn machine digest trace must be stable"
        );
        assert_eq!(
            first_trace, second_trace,
            "{label}: the (turn_counter, active team) trace must be stable"
        );
        assert_eq!(
            first_events, second_events,
            "{label}: the Turn notice stream must be identical on a repeated run"
        );

        // The machine sequence itself is pinned: the round entry starts RED at
        // turn 0 (the fixture's counter), the two teams alternate, and
        // `turn_counter` advances exactly once per closed round
        // (RED, GREEN, RED, GREEN, RED → 0, 0, 1, 1, 2).
        let red = TeamId::RED.0;
        let green = TeamId::GREEN.0;
        assert_eq!(
            first_trace,
            vec![(0, red), (0, green), (1, red), (1, green), (2, red)],
            "{label}: the local turn machine trace must be pinned"
        );

        // Not vacuous: both teams acted and at least one round closed.
        let notices: Vec<TurnNotice> = first_events
            .iter()
            .filter_map(|event| match event {
                GameEvent::Turn(notice) => Some(*notice),
                _ => None,
            })
            .collect();
        let acting: BTreeSet<u8> = notices
            .iter()
            .filter_map(|notice| match notice.event {
                TurnEvent::Human(team) | TurnEvent::Computer(team) => Some(team.0),
                TurnEvent::RoundClosed | TurnEvent::GameOver => None,
            })
            .collect();

        assert_eq!(
            acting.len(),
            2,
            "{label}: both duel teams must get a turn: {notices:?}"
        );
        assert!(
            notices
                .iter()
                .any(|notice| notice.event == TurnEvent::RoundClosed),
            "{label}: two full rounds must close a round: {notices:?}"
        );
    }
}

#[test]
fn apply_now_and_submit_tick_are_equivalent_for_every_ui_command() {
    let (fixture, red, green) = duel();
    let commands = ui_commands(&fixture, red, green);

    for (name, command) in commands {
        let (mut immediate, _, _) = duel();
        let (mut queued, _, _) = duel();

        // The immediate channel: the effect is visible before the first tick.
        let outcome = immediate.apply_now(command.clone());
        assert_eq!(
            outcome,
            ApplyOutcome::Applied,
            "{name}: the immediate channel must apply the command"
        );
        // The lockstep channel: the same command is queued for the next tick.
        assert_eq!(
            queued.submit(command),
            max_core::universe::SubmitOutcome::Accepted,
            "{name}: the lockstep channel must accept the command"
        );

        for tick in 1..=TICKS {
            immediate.tick();
            queued.tick();
            assert_eq!(
                immediate.snapshot().snapshot_bytes(),
                queued.snapshot().snapshot_bytes(),
                "{name}: state diverged after tick {tick} — the command is classified \
                 into the wrong channel (an accepted tick shift is not allowed here)"
            );
            assert_eq!(
                immediate.snapshot().state_digest(),
                queued.snapshot().state_digest(),
                "{name}: digest diverged after tick {tick}"
            );
        }
    }
}

#[test]
fn end_local_turn_is_not_the_queued_06() {
    let (mut immediate, _, _) = duel();
    let (mut queued, _, _) = duel();

    // B1: the turn close of the local/demo path is the facade's atomic operation.
    let over = immediate.end_local_turn(TeamId::RED);
    assert!(!over, "a two-team duel must continue");

    // The former path: packet `06` queued for the next tick.
    assert_eq!(
        queued.submit(max_core::events::command_finished_turn(TeamId::RED)),
        max_core::universe::SubmitOutcome::Accepted
    );
    queued.tick();

    assert_ne!(
        immediate.snapshot().snapshot_bytes(),
        queued.snapshot().snapshot_bytes(),
        "`end_local_turn` (immediate 06 + the machine) must stay distinguishable from \
         `submit(06)` + tick: a queued 06 enters EndTurn again on the next tick"
    );
}

/// The gate runs through the application, not only through the facade: the demo
/// HUD queue is applied by [`App::flush_orders`] (immediate) while the network
/// driver submits the same command — both must reach the same state.
#[test]
fn app_flush_orders_matches_a_queued_tick() {
    let (fixture, red, green) = duel();
    let commands = ui_commands(&fixture, red, green);

    for (name, command) in commands {
        let mut immediate = App::new(AppConfig::default());
        immediate.adopt_universe(duel().0);

        let mut queued = App::new(AppConfig::default());
        queued.adopt_universe(duel().0);

        immediate.hud_mut().pending.push(command.clone());
        assert_eq!(
            immediate.flush_orders(),
            1,
            "{name}: `App::flush_orders` must apply the queued command"
        );
        assert!(immediate.hud().pending.is_empty());
        assert_eq!(
            queued.submit(command),
            max_core::universe::SubmitOutcome::Accepted
        );

        for tick in 1..=TICKS {
            immediate.tick();
            queued.tick();
            assert_eq!(
                immediate.state_ref().snapshot_bytes(),
                queued.state_ref().snapshot_bytes(),
                "{name}: `App::flush_orders` vs `App::submit` diverged after tick {tick}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Packet 09 — the menu commits (SDL/UI Stage B, B2)
// ---------------------------------------------------------------------------

/// The duel fixture as an [`App`] with gold and the RED tank selected — the state
/// in which a player opens the upgrades screen.
fn economy_app() -> App {
    let mut app = App::new(AppConfig::default());
    app.adopt_universe(duel().0);
    app.universe_mut()
        .team_units_mut(TeamId::RED)
        .expect("RED economy")
        .gold = 500;
    let red = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state_ref().unit(id))
        .find(|unit| unit.team == TeamId::RED && unit.unit_type == UnitType::Tank)
        .map(|unit| unit.id)
        .expect("RED tank");
    app.hud_mut().select(Some(red));
    app
}

/// Runs `UpgradeMenu::Done` through the UI: buy one attack upgrade of `Infantry`.
fn commit_upgrade_ui(app: &mut App, channel: SessionChannel) -> Vec<Command> {
    assert!(app.open_upgrade_menu(), "upgrades screen opens");
    let index = app
        .upgrade_menu()
        .expect("menu")
        .unit_types
        .iter()
        .position(|unit_type| *unit_type == UnitType::Infantry)
        .expect("Infantry is listed");
    assert!(app.upgrade_menu_select(index));
    assert!(app.upgrade_menu_adjust(0, true), "attack upgrade is bought");
    assert!(app.commit_upgrade_menu(channel).is_some(), "commit closes the screen");

    app.take_outgoing_orders()
}

/// SDL/UI Stage B equivalence gate for the upgrades screen: the single-player
/// commit (`flush_orders` → `apply_now`) and the networked commit
/// (`Command::TeamState` broadcast, applied by the peer on its own queue) must
/// leave byte-identical state. A difference means the team economy still mutates
/// outside the command channel.
#[test]
fn upgrade_menu_commit_matches_the_broadcast_apply_on_a_peer() {
    // Single player: the UI commit flushes the packet immediately.
    let mut single = economy_app();
    assert!(commit_upgrade_ui(&mut single, SessionChannel::Local).is_empty(), "flush empties the queue");
    for _ in 0..TICKS {
        single.tick();
    }

    // Network: the same UI actions leave packet 09 (and 10) in the outgoing queue.
    let mut owner = economy_app();
    let commands = commit_upgrade_ui(&mut owner, SessionChannel::Lockstep);
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, Command::TeamState(_))),
        "the upgrade commit emits packet 09: {commands:?}"
    );

    let mut peer = economy_app();
    for command in commands.clone() {
        assert_eq!(
            owner.submit(command.clone()),
            max_core::universe::SubmitOutcome::Accepted
        );
        assert_eq!(
            peer.submit(command),
            max_core::universe::SubmitOutcome::Accepted,
            "the peer replays the same broadcast frames"
        );
    }

    for _ in 0..TICKS {
        owner.tick();
        peer.tick();
    }

    assert_eq!(
        single.state_ref().snapshot_bytes(),
        owner.state_ref().snapshot_bytes(),
        "single-player gold change must equal the networked owner's after the broadcast"
    );
    assert_eq!(
        owner.state_ref().snapshot_bytes(),
        peer.state_ref().snapshot_bytes(),
        "the broadcast must leave the peer byte-identical (09 is simulation state)"
    );
}

/// The duel fixture as an [`App`] with a powered-on research center of RED selected.
fn research_app() -> App {
    let mut app = App::new(AppConfig::default());
    app.adopt_universe(duel().0);
    let id = app
        .universe_mut()
        .spawn_unit(
            UnitType::ResearchCentre,
            TeamId::RED,
            Point::new(20, 20),
            0,
        )
        .expect("research center");
    let mut unit = app.state_ref().unit(id).cloned().expect("unit");
    unit.orders = UnitOrder::PowerOn;
    unit.state = UnitOrderState::ExecutingOrder;
    unit.research_topic = ResearchTopic::Attack;
    app.universe_mut().write_unit(id, unit);
    app.universe_mut()
        .update_research_progress(TeamId::RED, ResearchTopic::Attack, 1);
    app.hud_mut().select(Some(id));
    app
}

/// Runs `ResearchMenu::Done` through the UI: move the only center from Attack to
/// Shots.
fn commit_research_ui(app: &mut App, channel: SessionChannel) -> Vec<Command> {
    assert!(app.open_research_menu(), "research screen opens");
    assert!(
        app.research_menu_adjust(ResearchTopic::Attack as usize, false),
        "Attack releases the center"
    );
    assert!(
        app.research_menu_adjust(ResearchTopic::Shots as usize, true),
        "Shots takes the center"
    );
    assert!(app.commit_research_menu(channel).is_some(), "Done closes the screen");

    app.take_outgoing_orders()
}

/// SDL/UI Stage B equivalence gate for the research screen: the single-player
/// commit and the networked commit (packet 09 broadcast → peer apply) must leave
/// byte-identical state, including the per-center reassignment.
#[test]
fn research_menu_commit_matches_the_broadcast_apply_on_a_peer() {
    let mut single = research_app();
    assert!(commit_research_ui(&mut single, SessionChannel::Local).is_empty());
    for _ in 0..TICKS {
        single.tick();
    }

    let mut owner = research_app();
    let commands = commit_research_ui(&mut owner, SessionChannel::Lockstep);
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, Command::TeamState(_))),
        "the research commit emits packet 09: {commands:?}"
    );

    let mut peer = research_app();
    for command in commands.clone() {
        assert_eq!(
            owner.submit(command.clone()),
            max_core::universe::SubmitOutcome::Accepted
        );
        assert_eq!(
            peer.submit(command),
            max_core::universe::SubmitOutcome::Accepted
        );
    }

    for _ in 0..TICKS {
        owner.tick();
        peer.tick();
    }

    assert_eq!(
        single.state_ref().snapshot_bytes(),
        owner.state_ref().snapshot_bytes(),
        "single-player research change must equal the networked owner's"
    );
    assert_eq!(
        owner.state_ref().snapshot_bytes(),
        peer.state_ref().snapshot_bytes(),
        "the peer must reassign the center exactly like the owner (09 apply)"
    );

    let allocation = owner.state_ref().research_allocation(TeamId::RED);
    assert_eq!(allocation[ResearchTopic::Attack as usize], 0);
    assert_eq!(allocation[ResearchTopic::Shots as usize], 1);
}
