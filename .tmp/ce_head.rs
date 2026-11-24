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
//! next tick. [`end_local_turn_and_submit_06_are_distinguishable`] keeps the two
//! paths distinguishable, as the stage-A gate required.

use max_app::{App, SessionChannel};
use max_app::AppConfig;
use max_core::enums::{ResearchTopic, TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::events::{command_move_linear, command_order_p1, payload_p1_for, ApplyOutcome};
use max_core::geo::Point;
use max_core::input::{
    Command, OrderPayload, Packet08Order, Packet09TeamState, Packet10UpgradeUnitType, Packet18Chat,
    Packet41MoveFinished, Packet50StopUnit, PayloadP2, PayloadP3, PayloadP4, PayloadP5,
};
use max_core::research::upgrade_packet_values;
use max_core::team::{ResearchTopicInfo, TeamId, TeamKind, TeamUnits};
use max_core::unit::UnitId;
use max_core::universe::{GameUniverse, LocalDispatcher};

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

    (universe, red, green)
}

/// Every command kind the UI used to apply through `apply_now`.
///
/// The list mirrors the call sites of the demo path: the HUD order queue
/// (`38`/`08` P1–P5, `41`, `50`, `18`), the build menu/site (`08` P4) and the
/// upgrade screen (`10`). A new command kind that reaches `hud.pending` must be
/// added here: the gate is exhaustive for the UI paths, not a sample.
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
        immediate.universe = duel().0;

        let mut queued = App::new(AppConfig::default());
        queued.universe = duel().0;

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
    app.universe = duel().0;
    app.universe
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
    app.universe = duel().0;
    let id = app
        .universe
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
    app.universe.write_unit(id, unit);
    app.universe
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