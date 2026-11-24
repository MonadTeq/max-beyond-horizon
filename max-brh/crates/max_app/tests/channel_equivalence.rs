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

use max_app::App;
use max_app::AppConfig;
use max_core::enums::{TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::events::{command_move_linear, command_order_p1, payload_p1_for, ApplyOutcome};
use max_core::geo::Point;
use max_core::input::{
    Command, OrderPayload, Packet08Order, Packet10UpgradeUnitType, Packet18Chat,
    Packet41MoveFinished, Packet50StopUnit, PayloadP2, PayloadP3, PayloadP4, PayloadP5,
};
use max_core::research::upgrade_packet_values;
use max_core::team::{TeamId, TeamKind, TeamUnits};
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
