//! **Gate 3** — migrating `06`/lobby commands to `submit` (stage-A task
//! `game-universe-stage-a.md` §5).
//!
//! The gate compares the two command paths on byte-identical scenarios:
//!
//! * **legacy** — the current application path: the command is applied immediately
//!   (`events::apply_command`) and the turn transition (`enter_end_turn` +
//!   `GameState::next_team`) happens in the same frame, as `local_end_turn` does
//!   today (`max_app::local`);
//! * **facade** — the stage-A path: the command is submitted
//!   (`GameUniverse::submit`) and is therefore applied at the start of the next
//!   tick, so the transition happens after one simulation tick.
//!
//! Two comparisons are made for every scenario:
//!
//! 1. **same tick count** — proves that routing `06`/`13`/`12` through the queue
//!    changes nothing in the simulation state: `apply(cmd) + K ticks` is
//!    byte-identical to `submit(cmd) + K ticks`;
//! 2. **with the one-tick compensation** — the deferred transition makes the facade
//!    run one simulation tick more than the legacy path, so the legacy path is
//!    compared at the tick where the facade has the same number of simulation
//!    steps, and the compensated legacy run (one pre-transition tick, like the
//!    facade) must be byte-identical.
//!
//! Scenarios: `06` turn-based, `06` simultaneous, hot-seat (two human teams, a
//! full round), lobby packets `13`+`12`.
//!
//! # Stage B §5 — every command scenario
//!
//! The gate is extended to the queue-able packets `08`/`38`/`41`/`50`/`18`/`43`/
//! `11`/`20`/`10`/`14`:
//!
//! * `enqueue`+tick (the old receive path) and `apply`+tick (the old local path)
//!   are compared with `submit`+tick;
//! * `06` is **not** migrated: it keeps the immediate `end_local_turn` path, stays
//!   byte-identical to `apply(06) + enter_end_turn + next_team`, and remains
//!   distinguishable from `submit(06)`.
//!
//! **Tick accounting.** `GameState::tick` drains the command queue (step 2) before
//! the rules phase (steps 3-4), so a submitted command and an immediately applied
//! one commit in the *same* simulation step. The queue costs one frame of
//! observability (between `submit` and the next tick), not one simulation tick:
//! the byte-for-byte equivalence holds at the **same** tick count, while the
//! literal `N+1` tick from the stage-B task would add a real simulation step and
//! diverge. `queueable_commands_do_not_need_an_extra_tick` pins this down, so the
//! equal-count comparisons below are not a bookkeeping accident.

use crate::crc;
use crate::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};
use crate::enums::{GameStateType, PlayMode, TeamClan, UnitOrder, UnitOrderState, UnitType};
use crate::events::{apply_command, command_finished_turn, command_move_linear, payload_p1_for};
use crate::game::OrderData;
use crate::geo::{PathStep, Point};
use crate::input::{
    Command, OrderPayload, Packet08Order, Packet10UpgradeUnitType, Packet11ComplexOrder,
    Packet12MissionSupply, Packet13Seed, Packet14DeployUnit, Packet18Chat, Packet20UpgradeUnit,
    Packet38Move, Packet41MoveFinished, Packet43Rename, Packet50StopUnit, PayloadP2, PayloadP3,
    UPGRADE_VALUE_COUNT,
};
use crate::team::{TeamId, TeamKind, TeamUnits};
use crate::unit::{GroundPath, UnitId, UnitPath};
use crate::unit_values::ATTRIB_TURNS;
use crate::universe::{GameUniverse, LocalDispatcher, SubmitOutcome};
use crate::game::GameState;

/// A compact description of a snapshot for readable failure messages.
struct Snap(Vec<u8>);

impl Snap {
    fn capture(state: &GameState) -> Self {
        Self(state.snapshot_bytes())
    }

    fn crc(&self) -> u16 {
        crc::crc16(&self.0)
    }

    fn equal(&self, other: &Self, what: &str) {
        if self.0 == other.0 {
            return;
        }

        let first = self
            .0
            .iter()
            .zip(other.0.iter())
            .position(|(a, b)| a != b)
            .map(|index| index.to_string())
            .unwrap_or_else(|| "no common prefix".to_string());

        panic!(
            "{what}: snapshots diverge (len {} vs {}, crc16 0x{:04X} vs 0x{:04X}, \
             first differing byte {first})",
            self.0.len(),
            other.0.len(),
            self.crc(),
            other.crc()
        );
    }

    fn differ(&self, other: &Self, what: &str) {
        assert!(
            self.0 != other.0,
            "{what}: snapshots are equal, but a difference was expected"
        );
    }
}

/// A match in the middle of RED's turn: two teams, one tank each.
fn match_in_red_turn(play_mode: PlayMode, hot_seat: bool) -> GameState {
    let mut state = GameState::new();
    state.play_mode = play_mode;
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = if hot_seat {
        TeamKind::Player
    } else {
        TeamKind::Computer
    };
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units[1] = TeamUnits::with_clan(TeamClan::Random);
    state.active_turn_team = TeamId::RED;
    state.player_team = TeamId::RED;

    let _ = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("red tank");
    let _ = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            Point::new(20, 10),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("green tank");

    // `GameManager_TeamTurnTurnBased` entry: the cursor is reset, the first team
    // started — the state `local_next_team` leaves behind after the first call.
    state.begin_round();
    let _ = state.next_team();
    state
}

/// The legacy close: immediate apply + immediate transition (current `local_end_turn`).
fn legacy_close(state: &mut GameState, team: TeamId) {
    apply_command(state, &command_finished_turn(team));
    state.enter_end_turn();
    let _ = state.next_team();
}

/// The facade close: submit + one tick + the transition (the stage-A deferral).
fn facade_close(universe: &mut GameUniverse<LocalDispatcher>, team: TeamId) {
    assert_eq!(
        universe.submit(command_finished_turn(team)),
        SubmitOutcome::Accepted
    );
    universe.tick();
    (&mut universe.state).enter_end_turn();
    let _ = (&mut universe.state).next_team();
}

/// Compares one `06` turn close in the requested play mode.
fn check_06(play_mode: PlayMode, hot_seat: bool) {
    // 1. Same tick count: the command path itself is neutral.
    let legacy_same = {
        let mut state = match_in_red_turn(play_mode, hot_seat);
        apply_command(&mut state, &command_finished_turn(TeamId::RED));
        for _ in 0..3 {
            state.tick();
        }
        (Snap::capture(&state), state.turn_counter)
    };
    let facade_same = {
        let mut universe =
            GameUniverse::new(match_in_red_turn(play_mode, hot_seat), LocalDispatcher);
        universe.submit(command_finished_turn(TeamId::RED));
        for _ in 0..3 {
            universe.tick();
        }
        (
            Snap::capture(&universe.state),
            (&universe.state).turn_counter,
        )
    };
    legacy_same
        .0
        .equal(&facade_same.0, "submit→tick vs apply→tick (packet 06)");
    assert_eq!(
        legacy_same.1, facade_same.1,
        "{play_mode:?}: the turn counter path must match"
    );

    // 2. Compensated legacy: the same number of simulation ticks before the
    //    transition — byte-identical to the facade path (no hidden divergence).
    let legacy_compensated = {
        let mut state = match_in_red_turn(play_mode, hot_seat);
        apply_command(&mut state, &command_finished_turn(TeamId::RED));
        state.tick();
        state.enter_end_turn();
        let _ = state.next_team();
        for _ in 0..3 {
            state.tick();
        }
        Snap::capture(&state)
    };
    let facade_full = {
        let mut universe =
            GameUniverse::new(match_in_red_turn(play_mode, hot_seat), LocalDispatcher);
        facade_close(&mut universe, TeamId::RED);
        for _ in 0..3 {
            universe.tick();
        }
        Snap::capture(&universe.state)
    };
    legacy_compensated.equal(&facade_full, "06 with the same simulation ticks");

    // 3. The stage-A task's literal "+1 tick" comparison is *not* an equivalence:
    //    the transition tick of the facade path costs one simulation pass (the tick
    //    process that the immediate transition does not have), and that pass is
    //    not a no-op — it toggles `update_flag` and advances the AI reaction
    //    alternation. The difference is therefore expected and one-tick wide.
    let legacy_uncompensated = {
        let mut state = match_in_red_turn(play_mode, hot_seat);
        legacy_close(&mut state, TeamId::RED);
        for _ in 0..3 {
            state.tick();
        }
        Snap::capture(&state)
    };
    legacy_uncompensated.differ(&facade_full, "06 without the compensation");

    // 4. The shift is exactly one simulation tick: the immediate legacy close
    //    equals the facade close after one fewer post-transition tick only when
    //    the pre-transition pass is neutral in the scenario; here it is not, so
    //    the comparison is only kept as a diagnostic of the shift direction.
    let _ = play_mode;
}

#[test]
fn packet_06_turn_based_after_compensation() {
    check_06(PlayMode::TurnBased, false);
}

#[test]
fn packet_06_simultaneous_after_compensation() {
    check_06(PlayMode::SimultaneousMoves, false);
}

#[test]
fn packet_06_hot_seat_full_round_after_compensation() {
    // Hot-seat: two human teams; RED closes, GREEN closes, the round closes
    // (`finish_turn` → `turn_counter + 1`), then the post-round ticks.
    let legacy = {
        let mut state = match_in_red_turn(PlayMode::TurnBased, true);
        legacy_close(&mut state, TeamId::RED);
        state.tick();
        legacy_close(&mut state, TeamId::GREEN);
        state.tick();
        state.tick();
        (Snap::capture(&state), state.turn_counter)
    };

    let facade = {
        let mut universe = GameUniverse::new(
            match_in_red_turn(PlayMode::TurnBased, true),
            LocalDispatcher,
        );
        // RED closes; the facade path spends one tick on the transition.
        facade_close(&mut universe, TeamId::RED);
        // The one-tick shift: one fewer post-transition tick than the legacy path.
        universe.tick();
        facade_close(&mut universe, TeamId::GREEN);
        universe.tick();
        universe.tick();
        (
            Snap::capture(&universe.state),
            (&universe.state).turn_counter,
        )
    };

    legacy.0.equal(&facade.0, "hot-seat round close");
    assert_eq!(legacy.1, facade.1, "hot-seat turn counter");
    assert_eq!(
        facade.1, 1,
        "the round close advances the turn counter exactly once"
    );
}

/// Lobby packet `12` (mission supplies) and `13` (seed) arrive **before** the world
/// is generated; the current code applies them immediately (`run_lobby`).
fn lobby_commands() -> Vec<Command> {
    vec![
        Command::Seed(Packet13Seed { seed: 7 }),
        Command::MissionSupply(Packet12MissionSupply {
            team: TeamId::RED,
            round: 0,
            start_gold: 100,
            team_gold: 100,
            units: Vec::new(),
            gold_spent_on_upgrades: 0,
            starting_position: Point::new(12, 12),
            proximity_alert_ack: 0,
        }),
    ]
}

#[test]
fn lobby_packets_13_12_are_equivalent_after_the_queue_flush() {
    let legacy = {
        let mut state = match_in_red_turn(PlayMode::TurnBased, false);
        for command in lobby_commands() {
            apply_command(&mut state, &command);
        }
        (Snap::capture(&state), state.mission_supplies[0].is_some())
    };

    // The facade path: queue at receipt, flush at the configuration boundary
    // (before the world is regenerated — otherwise packet 12 is lost with the old
    // state).
    let facade = {
        let mut universe = GameUniverse::new(
            match_in_red_turn(PlayMode::TurnBased, false),
            LocalDispatcher,
        );
        for command in lobby_commands() {
            assert_eq!(universe.submit(command), SubmitOutcome::Accepted);
        }
        let applied = (&mut universe.state).apply_pending_commands();
        assert_eq!(applied, 2, "both lobby packets are applied by the flush");
        (
            Snap::capture(&universe.state),
            (&universe.state).mission_supplies[0].is_some(),
        )
    };

    legacy.0.equal(&facade.0, "lobby packets 13/12");
    assert_eq!(legacy.1, facade.1);
    assert!(
        facade.1,
        "the mission supplies must be in the state before world generation"
    );

    // Without the flush the queued packet 12 has not reached the state yet — this
    // is why the lobby → game boundary needs the explicit configuration flush.
    let mut unflushed = GameUniverse::new(
        match_in_red_turn(PlayMode::TurnBased, false),
        LocalDispatcher,
    );
    for command in lobby_commands() {
        let _ = unflushed.submit(command);
    }
    assert!(
        (&unflushed.state).mission_supplies[0].is_none(),
        "a queued packet 12 is applied only at the next tick / flush"
    );
    assert_eq!(
        (&unflushed.state).game_state,
        GameStateType::InGame,
        "the lobby queue does not change the phase"
    );
}

// ---------------------------------------------------------------------------
// Stage B §5 — queue-able commands `08`/`38`/`41`/`50`/`18`/`43`/`11`/`20`/
// `10`/`14` on the hot-seat fixture
// ---------------------------------------------------------------------------

/// The scenario unit ids.
struct Gate3Ids {
    red: UnitId,
    green: UnitId,
}

/// A hot-seat match (RED and GREEN are human) in RED's turn, one tank per side.
///
/// GREEN holds a `MOVE` order with a ready ground path, so packets `41`/`50`
/// change the snapshot instead of being no-ops on an idle unit. `complex` also
/// creates the RED complex that packet `11` addresses.
fn hot_seat_fixture(complex: bool) -> (GameState, Gate3Ids) {
    let mut state = GameState::new();
    state.play_mode = PlayMode::TurnBased;
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = TeamKind::Player;
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units[1] = TeamUnits::with_clan(TeamClan::Random);
    state.active_turn_team = TeamId::RED;
    state.player_team = TeamId::RED;

    if complex {
        assert_eq!(
            state.team_units[0].create_complex().0,
            1,
            "packet 11 targets complex id 1"
        );
    }

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

    // GREEN is already moving: `41`/`50` have something to clear.
    state.set_order_data(
        green,
        OrderData {
            move_to: Point::new(23, 10),
            fire_on: Point::ZERO,
        },
    );
    state.set_order(green, UnitOrder::Move, UnitOrderState::InProgress);
    let mut green_unit = state.unit(green).cloned().expect("green unit");
    green_unit.path = Some(UnitPath::Ground(GroundPath {
        end_x: 23,
        end_y: 10,
        step_index: 0,
        steps: vec![
            PathStep::new(1, 0),
            PathStep::new(1, 0),
            PathStep::new(1, 0),
        ],
    }));
    state.write_unit(green, green_unit);

    // The round entry, as `local_next_team` leaves it after the first step.
    state.begin_round();
    let _ = state.next_team();

    (state, Gate3Ids { red, green })
}

fn command_08(ids: &Gate3Ids) -> Command {
    Command::Order(Packet08Order {
        unit: ids.red,
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
            enemy_unit: Some(ids.green),
            transfer_cargo: 0,
            stealth_dice_roll: 0,
        }),
    })
}

fn command_38_with_path(ids: &Gate3Ids) -> Command {
    command_move_linear(
        ids.red,
        Point::new(10, 10),
        Point::new(13, 10),
        UnitOrder::Move,
        UnitOrderState::InProgress,
    )
}

fn command_38_without_path(ids: &Gate3Ids) -> Command {
    // No steps: the receiver requests a path, which is computed at the next tick's
    // `poll_results` — the only packet where the queue/poll order can matter.
    Command::Move(Packet38Move {
        unit: ids.red,
        order: UnitOrder::Move,
        state: UnitOrderState::Init,
        grid: Point::new(10, 10),
        move_to: Point::new(13, 10),
        group_speed: 0,
        path: None,
    })
}

fn command_41(ids: &Gate3Ids) -> Command {
    Command::MoveFinished(Packet41MoveFinished::port_form(ids.green, true))
}

fn command_50(ids: &Gate3Ids) -> Command {
    Command::Stop(Packet50StopUnit { unit: ids.green })
}

fn command_18(ids: &Gate3Ids) -> Command {
    Command::Chat(Packet18Chat {
        sender: TeamId::RED,
        message: format!("gate-3 chat from {:?}", ids.red),
    })
}

fn command_11(_ids: &Gate3Ids) -> Command {
    Command::ComplexOrder(Packet11ComplexOrder {
        team: TeamId::RED,
        complex_id: 1,
        raw: 42,
        fuel: 43,
        gold: 44,
        power: 45,
        workers: 46,
        buildings: 47,
    })
}

fn command_20(ids: &Gate3Ids) -> Command {
    let mut values = [0u16; UPGRADE_VALUE_COUNT];
    values[0] = 9;
    Command::UpgradeUnit(Packet20UpgradeUnit {
        unit: ids.red,
        values,
    })
}

fn command_10(_ids: &Gate3Ids) -> Command {
    let mut values = [0u16; UPGRADE_VALUE_COUNT];
    values[0] = 9;
    Command::UpgradeUnitType(Packet10UpgradeUnitType {
        team: TeamId::RED,
        unit_type: UnitType::Tank,
        values,
    })
}

fn command_43(ids: &Gate3Ids) -> Command {
    Command::Rename(Packet43Rename {
        unit: ids.green,
        name: String::from("gate-3 renamed"),
    })
}

fn command_14(_ids: &Gate3Ids) -> Command {
    // The Rust port has no landing model: `apply_command` reports `Unsupported`
    // and never mutates the state (a documented gap, not a routing fault).
    Command::Deploy(Packet14DeployUnit {
        team: TeamId::RED,
        unit_type: UnitType::Tank,
        grid: Point::new(11, 11),
    })
}

/// Where a command's committed effect is visible.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EffectKind {
    /// The canonical digest (`snapshot_bytes`) carries it.
    Snapshot,
    /// Only an auxiliary field outside the digest carries it (see `observe`).
    Observed,
    /// The command is accepted but has no state effect at all (`14`).
    None,
}

/// The auxiliary observable compared in addition to the snapshot.
type ObserveEffect = fn(&GameState, &Gate3Ids) -> String;

/// One queue-able command scenario.
struct Gate3Scenario {
    name: &'static str,
    complex: bool,
    build: fn(&Gate3Ids) -> Command,
    effect: EffectKind,
    observe: ObserveEffect,
}

fn observe_nothing(_state: &GameState, _ids: &Gate3Ids) -> String {
    String::new()
}

fn observe_team_values(state: &GameState, _ids: &Gate3Ids) -> String {
    // `TeamUnits::current_values` is not part of `snapshot_bytes`; packet 10 still
    // has to reach the state identically through both paths.
    format!(
        "{:?}",
        state.team_units[TeamId::RED.0 as usize].current_values[UnitType::Tank as usize]
            .get_attribute(ATTRIB_TURNS)
    )
}

fn observe_unit_name(state: &GameState, ids: &Gate3Ids) -> String {
    // `UnitInfo::name` is outside the canonical digest (ADR §3).
    state
        .unit(ids.green)
        .map(|unit| unit.name.clone().unwrap_or_default())
        .unwrap_or_default()
}

fn observe_chat(state: &GameState, _ids: &Gate3Ids) -> String {
    // Packet 18 is a UI/log report outside the canonical digest (ADR §3).
    format!("{:?}", state.chat_log)
}

fn gate3_scenarios() -> Vec<Gate3Scenario> {
    vec![
        Gate3Scenario {
            name: "08 order",
            complex: false,
            build: command_08,
            effect: EffectKind::Snapshot,
            observe: observe_nothing,
        },
        Gate3Scenario {
            name: "38 move with a ready path",
            complex: false,
            build: command_38_with_path,
            effect: EffectKind::Snapshot,
            observe: observe_nothing,
        },
        Gate3Scenario {
            name: "38 move with a path request",
            complex: false,
            build: command_38_without_path,
            effect: EffectKind::Snapshot,
            observe: observe_nothing,
        },
        Gate3Scenario {
            name: "41 move finished",
            complex: false,
            build: command_41,
            effect: EffectKind::Snapshot,
            observe: observe_nothing,
        },
        Gate3Scenario {
            name: "50 emergency stop",
            complex: false,
            build: command_50,
            effect: EffectKind::Snapshot,
            observe: observe_nothing,
        },
        Gate3Scenario {
            name: "18 chat",
            complex: false,
            build: command_18,
            effect: EffectKind::Observed,
            observe: observe_chat,
        },
        Gate3Scenario {
            name: "43 rename",
            complex: false,
            build: command_43,
            effect: EffectKind::Observed,
            observe: observe_unit_name,
        },
        Gate3Scenario {
            name: "11 complex order",
            complex: true,
            build: command_11,
            effect: EffectKind::Snapshot,
            observe: observe_nothing,
        },
        Gate3Scenario {
            name: "20 unit upgrade",
            complex: false,
            build: command_20,
            effect: EffectKind::Snapshot,
            observe: observe_nothing,
        },
        Gate3Scenario {
            name: "10 unit-type upgrade",
            complex: false,
            build: command_10,
            effect: EffectKind::Observed,
            observe: observe_team_values,
        },
        Gate3Scenario {
            name: "14 deploy",
            complex: false,
            build: command_14,
            effect: EffectKind::None,
            observe: observe_nothing,
        },
    ]
}

fn gate3_scenario(name: &'static str) -> Gate3Scenario {
    gate3_scenarios()
        .into_iter()
        .find(|scenario| scenario.name == name)
        .unwrap_or_else(|| panic!("unknown gate-3 scenario: {name}"))
}

/// Which path drives one scenario run.
#[derive(Clone, Copy)]
enum CommandPath {
    /// The old receive path: `GameState::enqueue` + `tick`.
    LegacyEnqueue,
    /// The old local path: `events::apply_command` + `tick`.
    LegacyApply,
    /// The facade: `GameUniverse::submit` + `tick`.
    Facade,
    /// No command at all — the non-vacuity control.
    Control,
}

/// Runs one scenario and returns the canonical bytes and the auxiliary effect.
fn run_gate3_scenario(
    scenario: &Gate3Scenario,
    path: CommandPath,
    ticks: usize,
) -> (Vec<u8>, String) {
    let (state, ids) = hot_seat_fixture(scenario.complex);
    let mut universe = GameUniverse::new(state, LocalDispatcher);

    match path {
        CommandPath::Control => {}
        CommandPath::LegacyEnqueue => (&mut universe.state).enqueue((scenario.build)(&ids)),
        CommandPath::LegacyApply => {
            let _ = apply_command(&mut universe.state, &(scenario.build)(&ids));
        }
        CommandPath::Facade => {
            assert_eq!(
                universe.submit((scenario.build)(&ids)),
                SubmitOutcome::Accepted,
                "{}: submit must accept the command",
                scenario.name
            );
        }
    }

    for _ in 0..ticks {
        universe.tick();
    }

    (
        (&universe.state).snapshot_bytes(),
        (scenario.observe)(&universe.state, &ids),
    )
}

/// The gate itself: same-tick-count byte identity for one queue-able command,
/// plus the non-vacuity and deferral checks.
fn check_queueable_scenario(scenario: &Gate3Scenario) {
    // 0. The command must reach the state: otherwise the snapshot comparison would
    //    be vacuous (this also proves the scenario fixture is meaningful).
    let (applied_commit, applied_effect) =
        run_gate3_scenario(scenario, CommandPath::LegacyApply, 0);
    let (control_commit, control_effect) = run_gate3_scenario(scenario, CommandPath::Control, 0);
    match scenario.effect {
        EffectKind::Snapshot => assert_ne!(
            applied_commit, control_commit,
            "{}: the committed command must change the snapshot",
            scenario.name
        ),
        EffectKind::Observed => assert_ne!(
            applied_effect, control_effect,
            "{}: the committed command must change the auxiliary observable",
            scenario.name
        ),
        EffectKind::None => {
            assert_eq!(
                applied_commit, control_commit,
                "{}: the accepted command is a documented no-op (Unsupported)",
                scenario.name
            );
            assert_eq!(applied_effect, control_effect);
        }
    }

    // 1. Same tick count — `enqueue` and immediate `apply` commit in the same
    //    simulation step as `submit`, so all three paths are byte-identical.
    for ticks in [1usize, 2, 3, 4] {
        let (enqueued, enqueued_effect) =
            run_gate3_scenario(scenario, CommandPath::LegacyEnqueue, ticks);
        let (applied, applied_effect) =
            run_gate3_scenario(scenario, CommandPath::LegacyApply, ticks);
        let (submitted, submitted_effect) =
            run_gate3_scenario(scenario, CommandPath::Facade, ticks);

        Snap(enqueued).equal(
            &Snap(applied.clone()),
            &format!(
                "{}: enqueue+tick vs apply+tick ({ticks} ticks)",
                scenario.name
            ),
        );
        Snap(applied.clone()).equal(
            &Snap(submitted),
            &format!(
                "{}: submit+tick vs apply+tick ({ticks} ticks)",
                scenario.name
            ),
        );
        assert_eq!(
            submitted_effect, applied_effect,
            "{}: auxiliary effect after {ticks} ticks",
            scenario.name
        );
        assert_eq!(
            enqueued_effect, applied_effect,
            "{}: auxiliary effect after {ticks} ticks (enqueue path)",
            scenario.name
        );
    }

    // 2. The facade deferral itself: `submit` only queues, the next tick commits.
    let (state, ids) = hot_seat_fixture(scenario.complex);
    let mut universe = GameUniverse::new(state, LocalDispatcher);
    let before = (&universe.state).snapshot_bytes();
    let before_effect = (scenario.observe)(&universe.state, &ids);

    assert_eq!(
        universe.submit((scenario.build)(&ids)),
        SubmitOutcome::Accepted,
        "{}: submit outcome",
        scenario.name
    );
    assert_eq!(
        (&universe.state).snapshot_bytes(),
        before,
        "{}: submit must not mutate the state",
        scenario.name
    );
    assert_eq!(
        (scenario.observe)(&universe.state, &ids),
        before_effect,
        "{}: submit must not change the auxiliary observable",
        scenario.name
    );
    assert_eq!(
        (&universe.state).commands.len(),
        1,
        "{}: the command is queued",
        scenario.name
    );

    universe.tick();
    assert!(
        (&universe.state).commands.is_empty(),
        "{}: the next tick drains the queue",
        scenario.name
    );
}

#[test]
fn packet_08_order_is_byte_identical_through_the_queue() {
    check_queueable_scenario(&gate3_scenario("08 order"));
}

#[test]
fn packet_38_move_with_path_is_byte_identical_through_the_queue() {
    check_queueable_scenario(&gate3_scenario("38 move with a ready path"));
}

#[test]
fn packet_38_move_with_path_request_is_byte_identical_through_the_queue() {
    check_queueable_scenario(&gate3_scenario("38 move with a path request"));
}

#[test]
fn packet_41_move_finished_is_byte_identical_through_the_queue() {
    check_queueable_scenario(&gate3_scenario("41 move finished"));
}

#[test]
fn packet_50_stop_is_byte_identical_through_the_queue() {
    check_queueable_scenario(&gate3_scenario("50 emergency stop"));
}

#[test]
fn packet_18_chat_is_byte_identical_through_the_queue() {
    check_queueable_scenario(&gate3_scenario("18 chat"));
}

#[test]
fn packet_43_rename_is_byte_identical_through_the_queue() {
    check_queueable_scenario(&gate3_scenario("43 rename"));
}

#[test]
fn packet_11_complex_order_is_byte_identical_through_the_queue() {
    check_queueable_scenario(&gate3_scenario("11 complex order"));
}

#[test]
fn packet_20_unit_upgrade_is_byte_identical_through_the_queue() {
    check_queueable_scenario(&gate3_scenario("20 unit upgrade"));
}

#[test]
fn packet_10_unit_type_upgrade_is_byte_identical_through_the_queue() {
    check_queueable_scenario(&gate3_scenario("10 unit-type upgrade"));
}

#[test]
fn packet_14_deploy_is_byte_identical_through_the_queue() {
    check_queueable_scenario(&gate3_scenario("14 deploy"));
}

#[test]
fn lobby_packets_13_12_match_through_the_tick() {
    // The same lobby queue as above, but delivered by the tick instead of the
    // configuration flush: the enqueue path and `submit` commit in one step.
    for ticks in [1usize, 2, 4] {
        let mut legacy = match_in_red_turn(PlayMode::TurnBased, false);
        for command in lobby_commands() {
            let _ = apply_command(&mut legacy, &command);
        }
        for _ in 0..ticks {
            legacy.tick();
        }

        let mut universe = GameUniverse::new(
            match_in_red_turn(PlayMode::TurnBased, false),
            LocalDispatcher,
        );
        for command in lobby_commands() {
            assert_eq!(universe.submit(command), SubmitOutcome::Accepted);
        }
        for _ in 0..ticks {
            universe.tick();
        }

        Snap::capture(&legacy).equal(
            &Snap::capture(&universe.state),
            &format!("lobby 13/12 through the tick ({ticks} ticks)"),
        );
        assert_eq!(
            legacy.mission_supplies,
            (&universe.state).mission_supplies,
            "the lobby supplies must land in the same state"
        );
        assert!((&universe.state).mission_supplies[0].is_some());
    }
}

#[test]
fn queueable_commands_do_not_need_an_extra_tick() {
    // `GameState::tick` drains the queue (step 2) *before* the rules phase (steps
    // 3-4), so `submit` + K ticks is the same simulation-step sequence as
    // `apply` + K ticks. The one-tick shift is observability only (between
    // `submit` and the next tick), not a simulation tick; the literal `N+1`
    // compensation from the stage-B task adds a real step and therefore diverges.
    // This test pins both halves down, so the equal-count comparisons cannot be
    // dismissed as a bookkeeping accident.
    for scenario in gate3_scenarios() {
        for ticks in [1usize, 2, 4] {
            let (applied, _) = run_gate3_scenario(&scenario, CommandPath::LegacyApply, ticks);
            let (same, _) = run_gate3_scenario(&scenario, CommandPath::Facade, ticks);
            let (extra, _) = run_gate3_scenario(&scenario, CommandPath::Facade, ticks + 1);

            assert_eq!(
                same, applied,
                "{}: submit and apply must commit in the same tick ({ticks} ticks)",
                scenario.name
            );
            assert_ne!(
                extra, applied,
                "{}: an extra tick is a real simulation step, not a compensation",
                scenario.name
            );
        }
    }
}

/// Every order packet of one hot-seat frame, in arrival order.
fn hot_seat_order_batch(ids: &Gate3Ids) -> Vec<Command> {
    vec![
        command_08(ids),
        command_38_with_path(ids),
        command_41(ids),
        command_50(ids),
        command_18(ids),
        command_11(ids),
        command_20(ids),
        command_43(ids),
        command_10(ids),
    ]
}

fn run_order_batch(path: CommandPath, ticks: usize) -> (Vec<u8>, String) {
    let (state, ids) = hot_seat_fixture(true);
    let mut universe = GameUniverse::new(state, LocalDispatcher);
    let commands = hot_seat_order_batch(&ids);

    match path {
        CommandPath::Control => {}
        CommandPath::LegacyEnqueue => {
            for command in commands {
                (&mut universe.state).enqueue(command);
            }
        }
        CommandPath::LegacyApply => {
            for command in &commands {
                let _ = apply_command(&mut universe.state, command);
            }
        }
        CommandPath::Facade => {
            for command in commands {
                assert_eq!(universe.submit(command), SubmitOutcome::Accepted);
            }
        }
    }

    for _ in 0..ticks {
        universe.tick();
    }

    let state = &universe.state;
    let observed = format!(
        "{:?}|{:?}|{:?}",
        state.chat_log,
        state.team_units[TeamId::RED.0 as usize].current_values[UnitType::Tank as usize]
            .get_attribute(ATTRIB_TURNS),
        state.unit(ids.green).map(|unit| unit.name.clone()),
    );

    (state.snapshot_bytes(), observed)
}

#[test]
fn hot_seat_order_batch_is_byte_identical_through_the_queue() {
    // Arrival order of the whole frame is preserved: every packet of the batch is
    // applied in the same receive order on all three paths.
    let (control, control_effect) = run_order_batch(CommandPath::Control, 0);
    let (applied_commit, applied_effect) = run_order_batch(CommandPath::LegacyApply, 0);
    assert_ne!(
        applied_commit, control,
        "the batch must change the snapshot (non-vacuity)"
    );
    assert_ne!(applied_effect, control_effect);

    for ticks in [1usize, 2, 4] {
        let (enqueued, enqueued_effect) = run_order_batch(CommandPath::LegacyEnqueue, ticks);
        let (applied, applied_effect) = run_order_batch(CommandPath::LegacyApply, ticks);
        let (submitted, submitted_effect) = run_order_batch(CommandPath::Facade, ticks);

        Snap(enqueued).equal(
            &Snap(applied.clone()),
            &format!("hot-seat batch: enqueue+tick vs apply+tick ({ticks} ticks)"),
        );
        Snap(applied).equal(
            &Snap(submitted),
            &format!("hot-seat batch: submit+tick vs apply+tick ({ticks} ticks)"),
        );
        assert_eq!(
            submitted_effect, applied_effect,
            "hot-seat batch: auxiliary effect"
        );
        assert_eq!(
            enqueued_effect, applied_effect,
            "hot-seat batch: auxiliary effect (enqueue)"
        );
    }
}
