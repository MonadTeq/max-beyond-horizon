use super::*;
use crate::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};
use crate::enums::TeamClan;
use crate::input::ComplexSummaryEntry;
use crate::input::{
    Packet00TurnBarrier, Packet01FrameBarrier, Packet13Seed, Packet18Chat,
    Packet41MoveFinished, Packet45CrcBarrier, Packet50StopUnit, Packet52EndTurnBarrier,
};
use crate::team::{ResearchTopicInfo, TeamKind};
use crate::unit_values::ATTRIB_HITS;

fn state_with_tank() -> (GameState, UnitId) {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);
    let id = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            Point::new(10, 10),
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("spawn");
    (state, id)
}

#[test]
fn command_queue_is_fifo() {
    let (mut state, id) = state_with_tank();
    state
        .commands
        .push(Command::Stop(Packet50StopUnit { unit: id }));
    state.commands.push(Command::Seed(Packet13Seed { seed: 7 }));
    assert_eq!(state.commands.len(), 2);

    assert_eq!(state.apply_pending_commands(), 2);
    assert!(state.commands.is_empty());
    assert_eq!(state.commands.applied(), 2);
    assert_eq!(state.unit_events.len(), 1);
    assert_eq!(state.rng, crate::rng::Randomizer::new(7));
}

#[test]
fn packet_08_p1_sets_orders_and_validates_range() {
    let (mut state, id) = state_with_tank();
    let p1 = payload_p1_for(
        UnitOrder::Sentry,
        UnitOrderState::ExecutingOrder,
        UnitOrder::Await,
        UnitOrderState::ExecutingOrder,
    );
    assert_eq!(
        apply_command(&mut state, &command_order_p1(id, p1)),
        ApplyOutcome::Applied
    );
    let unit = state.unit(id).unwrap();
    assert_eq!(unit.orders, UnitOrder::Sentry);
    assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
    assert_eq!(unit.prior_orders, UnitOrder::Await);
}

#[test]
fn packet_08_p3_applies_targets_and_payload_level_mismatch_is_malformed() {
    let (mut state, id) = state_with_tank();
    let p1 = payload_p1_for(
        UnitOrder::Move,
        UnitOrderState::Init,
        UnitOrder::Await,
        UnitOrderState::ExecutingOrder,
    );
    let p3 = PayloadP3 {
        p2: PayloadP2 {
            p1,
            parent_unit: None,
            experience: 5,
        },
        move_to: Point::new(12, 13),
        fire_on: Point::new(14, 15),
        enemy_unit: None,
        transfer_cargo: 3,
        stealth_dice_roll: 9,
    };
    let command = Command::Order(Packet08Order {
        unit: id,
        payload: OrderPayload::P3(p3),
    });
    assert_eq!(apply_command(&mut state, &command), ApplyOutcome::Applied);

    let unit = state.unit(id).unwrap();
    assert_eq!(unit.experience, 5);
    assert_eq!(unit.transfer_cargo, 3);
    assert_eq!(unit.stealth_dice_roll, 9);
    assert_eq!(unit.target_grid, Point::new(12, 13));
    let data = state.order_data(id);
    assert_eq!(data.move_to, Point::new(12, 13));
    assert_eq!(data.fire_on, Point::new(14, 15));

    // P3 payload with a P1-level order — malformed.
    let malformed = Command::Order(Packet08Order {
        unit: id,
        payload: OrderPayload::P3(PayloadP3 {
            p2: PayloadP2 {
                p1: payload_p1_for(
                    UnitOrder::Sentry,
                    UnitOrderState::Init,
                    UnitOrder::Await,
                    UnitOrderState::ExecutingOrder,
                ),
                parent_unit: None,
                experience: 0,
            },
            move_to: Point::ZERO,
            fire_on: Point::ZERO,
            enemy_unit: None,
            transfer_cargo: 0,
            stealth_dice_roll: 0,
        }),
    });
    assert_eq!(
        apply_command(&mut state, &malformed),
        ApplyOutcome::Malformed
    );
}

#[test]
fn packet_38_builds_ground_path() {
    let (mut state, id) = state_with_tank();
    let command = command_move_linear(
        id,
        Point::new(10, 10),
        Point::new(13, 10),
        UnitOrder::Move,
        UnitOrderState::Init,
    );
    assert_eq!(apply_command(&mut state, &command), ApplyOutcome::Applied);
    let unit = state.unit(id).unwrap();
    match unit.path.as_ref().expect("path") {
        UnitPath::Ground(path) => {
            assert_eq!(path.end_x, 13);
            assert_eq!(path.end_y, 10);
            assert_eq!(path.steps.len(), 3);
            assert_eq!(path.step_index, 0);
        }
        other => panic!("ожидался наземный путь, получено {other:?}"),
    }
    assert_eq!(state.order_data(id).move_to, Point::new(13, 10));
}

#[test]
fn packet_41_stops_movement_and_restores_await() {
    let (mut state, id) = state_with_tank();
    let command = command_move_linear(
        id,
        Point::new(10, 10),
        Point::new(20, 10),
        UnitOrder::Move,
        UnitOrderState::InProgress,
    );
    apply_command(&mut state, &command);
    assert!(state.unit(id).unwrap().path.is_some());

    let stop = Command::MoveFinished(Packet41MoveFinished {
        unit: id,
        mode: true,
        grid: Some(Point::new(11, 10)),
        speed: Some(5),
        group_speed: Some(0),
        shots: Some(2),
        move_fraction: Some(1),
    });
    assert_eq!(apply_command(&mut state, &stop), ApplyOutcome::Applied);

    let unit = state.unit(id).unwrap();
    assert!(unit.path.is_none());
    assert_eq!(unit.orders, UnitOrder::Await);
    assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
    assert_eq!(unit.grid, Point::new(11, 10));
    assert_eq!(unit.speed, 5);
    assert_eq!(unit.shots, 2);
    assert_eq!(unit.delayed_reaction, 1);
}

#[test]
fn packet_50_creates_emergency_stop_event_processed_by_cycle() {
    let (mut state, id) = state_with_tank();
    // Start moving.
    let command = command_move_linear(
        id,
        Point::new(10, 10),
        Point::new(10, 15),
        UnitOrder::Move,
        UnitOrderState::InProgress,
    );
    apply_command(&mut state, &command);
    state.unit_mut(id).unwrap().state = UnitOrderState::InProgress;

    assert_eq!(
        apply_command(&mut state, &Command::Stop(Packet50StopUnit { unit: id })),
        ApplyOutcome::Applied
    );
    assert_eq!(state.unit_events.len(), 1);
    assert!(
        state.unit(id).unwrap().path.is_some(),
            "событие ещё не обработано"
    );

    state.tick();
    assert!(
        state.unit_events.is_empty(),
            "очередь событий очищается за тик"
    );
    assert!(state.unit(id).unwrap().path.is_none());
}

#[test]
fn packet_06_sets_finished_turn_and_end_state() {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.play_mode = crate::enums::PlayMode::TurnBased;
    assert_eq!(
        apply_command(&mut state, &command_finished_turn(TeamId::RED)),
        ApplyOutcome::Applied
    );
    assert!(state.teams[0].finished_turn);
    assert_eq!(state.game_state, crate::enums::GameStateType::EndTurn);

    // An invalid team index (ALIEN=4) is rejected.
    assert_eq!(
        apply_command(&mut state, &command_finished_turn(TeamId::ALIEN)),
        ApplyOutcome::Ignored
    );
}

#[test]
fn barriers_are_recorded_not_simulated() {
    let mut state = GameState::new();
    apply_command(&mut state, &command_end_turn_barrier(TeamId::GREEN, 7));
    assert_eq!(state.turn_indices[1], 7);
    apply_command(
        &mut state,
        &Command::CrcBarrier(Packet45CrcBarrier {
            team: TeamId::GREEN,
            next_turn_index: 3,
            crc16: 0xABCD,
        }),
    );
    assert_eq!(state.next_turn_indices[1], 3);
    assert_eq!(state.team_data_crc16[1], 0xABCD);
    assert!(!Command::EndTurnBarrier(Packet52EndTurnBarrier {
        team: TeamId::RED,
        counter: 1
    })
    .affects_simulation());
}

#[test]
fn upgrade_packets_update_attributes_in_protocol_order() {
    let (mut state, id) = state_with_tank();
    let mut values = [0u16; UPGRADE_VALUE_COUNT];
    values[1] = 99; // hits — the second attribute in packet order.

    apply_command(
        &mut state,
        &Command::UpgradeUnit(crate::input::Packet20UpgradeUnit { unit: id, values }),
    );
    assert_eq!(
        state
            .unit(id)
            .unwrap()
            .base_values
            .get_attribute(ATTRIB_HITS),
        99
    );

    apply_command(
        &mut state,
        &Command::UpgradeUnitType(crate::input::Packet10UpgradeUnitType {
            team: TeamId::RED,
            unit_type: UnitType::Tank,
            values,
        }),
    );
    assert_eq!(
        state.team_units[0]
            .current_unit_values(UnitType::Tank as u16)
            .get_attribute(ATTRIB_HITS),
        99
    );
}

#[test]
fn complex_packets_apply_six_fields() {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    let complex_id = state.team_units[0].create_complex();
    apply_command(
        &mut state,
        &Command::ComplexOrder(crate::input::Packet11ComplexOrder {
            team: TeamId::RED,
            complex_id: complex_id.0 as u16,
            raw: 5,
            fuel: 6,
            gold: 7,
            power: 8,
            workers: 9,
            buildings: 10,
        }),
    );
    let complex = state
        .complex(TeamId::RED, complex_id.0 as u16)
        .expect("complex");
    assert_eq!(complex.raw, 5);
    assert_eq!(complex.buildings, 10);

    apply_command(
        &mut state,
        &Command::ComplexSummary(crate::input::Packet53ComplexSummary {
            team: TeamId::RED,
            turn: 3,
            complexes: vec![ComplexSummaryEntry {
                complex_id: complex_id.0 as u16,
                values: [1, 2, 3, 4, 5, 6],
            }],
        }),
    );
    let complex = state
        .complex(TeamId::RED, complex_id.0 as u16)
        .expect("complex");
    assert_eq!(complex.raw, 1);
    assert_eq!(complex.buildings, 6);
}

#[test]
fn deploy_is_reported_unsupported_until_unit_flag_data_exists() {
    let mut state = GameState::new();
    let outcome = apply_command(
        &mut state,
        &Command::Deploy(crate::input::Packet14DeployUnit {
            team: TeamId::RED,
            unit_type: UnitType::Tank,
            grid: Point::new(4, 4),
        }),
    );
    assert_eq!(outcome, ApplyOutcome::Unsupported);
}

#[test]
fn rename_and_unknown_unit() {
    let (mut state, id) = state_with_tank();
    apply_command(
        &mut state,
        &Command::Rename(crate::input::Packet43Rename {
            unit: id,
            name: "Tank 1".into(),
        }),
    );
    assert_eq!(state.unit(id).unwrap().name.as_deref(), Some("Tank 1"));

    // `UnitInfo::SetName("")` clears the own name — the C++ `char*` becomes `nullptr`.
    apply_command(
        &mut state,
        &Command::Rename(crate::input::Packet43Rename {
            unit: id,
            name: String::new(),
        }),
    );
    assert_eq!(state.unit(id).unwrap().name, None);

    assert_eq!(
        apply_command(
            &mut state,
            &Command::Rename(crate::input::Packet43Rename {
                unit: UnitId(0x7FFF),
                name: "x".into(),
            })
        ),
        ApplyOutcome::UnknownUnit
    );
}

#[test]
fn chat_is_logged() {
    let mut state = GameState::new();
    apply_command(
        &mut state,
        &Command::Chat(crate::input::Packet18Chat {
            sender: TeamId::BLUE,
            message: "hello".into(),
        }),
    );
    assert_eq!(state.chat_log.len(), 1);
    assert_eq!(state.chat_log[0].0, TeamId::BLUE);
}

#[test]
fn unit_event_helpers_are_consistent() {
    let event = UnitEvent::EmergencyStop { unit: UnitId(5) };
    assert_eq!(event.unit(), UnitId(5));
    assert_eq!(unit_type_from_raw(0x33), Some(UnitType::Tank));
    assert_eq!(unit_type_from_raw(0xFFFF), None);
    assert_eq!(research_topic_at(0), Some(ResearchTopic::Attack));
    assert_eq!(research_topic_at(8), None);
}

// -----------------------------------------------------------------------
// 54 — applying the authoritative team state
// -----------------------------------------------------------------------

fn state_with_remote_team() -> (GameState, UnitId, UnitId, UnitId) {
    let mut state = GameState::new();
    let green = TeamId::GREEN.0 as usize;
    state.teams[green].team_type = TeamKind::Remote;
    state.team_units[green] = crate::team::TeamUnits::with_clan(TeamClan::Random);

    let tank = state
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(5, 5), 0)
        .expect("tank");
    let scout = state
        .spawn_unit(UnitType::Scout, TeamId::GREEN, Point::new(6, 5), 0)
        .expect("scout");
    let tower = state
        .spawn_unit(UnitType::GoldRefinery, TeamId::GREEN, Point::new(8, 8), 0)
        .expect("tower");
    (state, tank, scout, tower)
}

fn unit_state_for_tank() -> UnitStatePacket {
    UnitStatePacket {
        orders: UnitOrder::Sentry as u8,
        state: UnitOrderState::ExecutingOrder as u8,
        prior_orders: UnitOrder::Await as u8,
        prior_state: UnitOrderState::ExecutingOrder as u8,
        disabled_reaction_fire: true,
        parent_unit_id: 0,
        experience: 11,
        move_to_grid_x: 50,
        move_to_grid_y: 51,
        fire_on_grid_x: 52,
        fire_on_grid_y: 53,
        enemy_unit_id: 0,
        transfer_cargo: -4,
        stealth_dice_roll: 3,
        total_mining: 1,
        raw_mining: 2,
        fuel_mining: 3,
        gold_mining: 4,
        build_time: 5,
        build_rate: 6,
        unit_type: UnitType::Tank as u16,
        unit_id: 7,
        grid_x: 40,
        grid_y: 41,
        team: TeamId::GREEN.0,
        hits: 77,
        speed: 8,
        shots: 2,
        storage: -9,
        ammo: 10,
    }
}

fn tail_for_green() -> TeamStateSyncTail {
    let mut tail = TeamStateSyncTail::default();
    tail.gold = 500;
    tail.gold_spent_on_upgrades = 60;
    tail.number_of_objects_created = 12;
    tail.unit_counters[UnitType::Tank as usize] = 13;
    tail.research[0] = ResearchTopicInfo {
        research_level: 1,
        turns_to_complete: 2,
        allocation: 3,
    };
    tail
}

#[test]
fn packet_54_applies_state_and_removes_extra_units() {
    let (mut state, tank, scout, tower) = state_with_remote_team();
    let packet = StateSyncPacket {
        team: TeamId::GREEN,
        sync_index: 1,
        is_last: true,
        units: vec![(tank, unit_state_for_tank())],
        tail: Some(tail_for_green()),
    };

    assert_eq!(apply_state_sync(&mut state, &packet), ApplyOutcome::Applied);

    let unit = state.unit(tank).expect("tank survives");
    assert_eq!(unit.orders, UnitOrder::Sentry);
    assert_eq!(unit.state, UnitOrderState::ExecutingOrder);
    assert!(unit.disabled_reaction_fire);
    assert_eq!(unit.grid, Point::new(40, 41));
    assert_eq!(unit.hits, 77);
    assert_eq!(unit.speed, 8);
    assert_eq!(unit.shots, 2);
    assert_eq!(unit.storage, -9);
    assert_eq!(unit.ammo, 10);
    assert_eq!(unit.experience, 11);
    assert_eq!(unit.build_rate, 6);
    assert_eq!(unit.unit_id, 7);
    let data = state.order_data(tank);
    assert_eq!(data.move_to, Point::new(50, 51));
    assert_eq!(data.fire_on, Point::new(52, 53));
    assert_eq!(unit.target_grid, Point::new(50, 51));

    // An extra mobile unit is removed, a building is not (`Remote_RemoveExtraUnits`).
    assert!(
        !state.has_unit(scout),
            "scout отсутствует у владельца — удалён"
    );
    assert!(state.has_unit(tower), "здание не удаляется");

    // The last chunk's tail aligned the economy and id counters.
    let green = TeamId::GREEN.0 as usize;
    assert_eq!(state.team_units[green].gold, 500);
    assert_eq!(state.teams[green].stats_gold_spent_on_upgrades, 60);
    assert_eq!(state.teams[green].number_of_objects_created, 12);
    assert_eq!(
        state.teams[green].team_unit_counters[UnitType::Tank as usize],
        13
    );
    assert_eq!(state.teams[green].research_topics[0].research_level, 1);
    assert_eq!(state.teams[green].research_topics[0].turns_to_complete, 2);
    assert_eq!(state.teams[green].research_topics[0].allocation, 3);
    assert_eq!(state.state_sync_index(TeamId::GREEN), 1);
}

#[test]
fn packet_54_chunks_only_complete_on_last() {
    let (mut state, tank, _scout, _tower) = state_with_remote_team();
    let first = StateSyncPacket {
        team: TeamId::GREEN,
        sync_index: 3,
        is_last: false,
        units: vec![(tank, unit_state_for_tank())],
        tail: None,
    };
    assert_eq!(apply_state_sync(&mut state, &first), ApplyOutcome::Applied);
    assert_eq!(
        state.state_sync_index(TeamId::GREEN),
        0,
            "не последний чанк"
    );

    let last = StateSyncPacket {
        team: TeamId::GREEN,
        sync_index: 3,
        is_last: true,
        units: Vec::new(),
        tail: Some(TeamStateSyncTail::default()),
    };
    assert_eq!(apply_state_sync(&mut state, &last), ApplyOutcome::Applied);
    assert_eq!(state.state_sync_index(TeamId::GREEN), 3);
}

#[test]
fn packet_54_ignores_non_remote_teams_and_out_of_range_orders() {
    // An own (Player) team: packet 54 is not applied.
    let (mut state, tank, _scout, _tower) = state_with_remote_team();
    state.teams[TeamId::RED.0 as usize].team_type = TeamKind::Player;
    let packet = StateSyncPacket {
        team: TeamId::RED,
        sync_index: 1,
        is_last: true,
        units: vec![(tank, unit_state_for_tank())],
        tail: Some(TeamStateSyncTail::default()),
    };
    assert_eq!(apply_state_sync(&mut state, &packet), ApplyOutcome::Ignored);
    assert_eq!(state.state_sync_index(TeamId::RED), 0);

    // An order outside ORDER_COUNT_MAX is not applied (`Remote_ApplyUnitState`).
    let before = state.unit(tank).expect("tank").orders;
    let mut bad = unit_state_for_tank();
    bad.orders = crate::enums::ORDER_COUNT_MAX;
    let packet = StateSyncPacket {
        team: TeamId::GREEN,
        sync_index: 4,
        is_last: true,
        units: vec![(tank, bad)],
        tail: Some(TeamStateSyncTail::default()),
    };
    assert_eq!(apply_state_sync(&mut state, &packet), ApplyOutcome::Applied);
    let unit = state.unit(tank).expect("tank");
    assert_eq!(unit.orders, before, "невалидный приказ не применён");
    assert_eq!(state.state_sync_index(TeamId::GREEN), 4);
}

// -----------------------------------------------------------------------
// Phase 1: dispatcher boundaries (U)
// -----------------------------------------------------------------------

/// An empty queue applies nothing and does not disturb the state;
/// `pop_front` on an empty queue is `None` and keeps the counter at 0.
#[test]
fn empty_command_queue_is_a_no_op() {
    let (mut state, _id) = state_with_tank();
    let before = state.snapshot_bytes();

    assert_eq!(apply_pending(&mut state), 0);
    assert_eq!(state.commands.applied(), 0);
    assert_eq!(state.snapshot_bytes(), before);

    let mut queue = CommandQueue::new();
    assert!(queue.is_empty());
    assert_eq!(queue.len(), 0);
    assert_eq!(queue.pop_front(), None);
    assert_eq!(queue.applied(), 0);
}

/// `CommandQueue::clear` drops pending commands but keeps the applied
/// diagnostic counter (`Remote_ReceiveNetPackets` drain semantics).
#[test]
fn clear_keeps_the_applied_counter() {
    let (mut state, id) = state_with_tank();
    state
        .commands
        .push(Command::Stop(Packet50StopUnit { unit: id }));
    state.commands.push(Command::Seed(Packet13Seed { seed: 1 }));

    assert!(state.commands.pop_front().is_some());
    assert_eq!(state.commands.applied(), 1);

    state.commands.clear();
    assert!(state.commands.is_empty());
    assert_eq!(state.commands.len(), 0);
    assert_eq!(
        state.commands.applied(),
        1,
            "the applied counter is not reset by clear"
    );
}

/// Commands for units that do not exist are rejected with `UnknownUnit`
/// and leave the canonical state untouched (`Remote_NetErrorUnknownUnit`).
#[test]
fn unknown_unit_is_rejected_without_changing_the_state() {
    let (mut state, _id) = state_with_tank();
    let missing = UnitId(0x7FFF);
    let before = state.snapshot_bytes();

    let commands = [
        Command::Order(crate::input::Packet08Order {
            unit: missing,
            payload: OrderPayload::P1(payload_p1_for(
                UnitOrder::Sentry,
                UnitOrderState::ExecutingOrder,
                UnitOrder::Await,
                UnitOrderState::ExecutingOrder,
            )),
        }),
        Command::Stop(Packet50StopUnit { unit: missing }),
        Command::MoveFinished(Packet41MoveFinished::port_form(missing, true)),
        Command::Rename(crate::input::Packet43Rename {
            unit: missing,
            name: "ghost".into(),
        }),
    ];

    for command in &commands {
        assert_eq!(
            apply_command(&mut state, command),
            ApplyOutcome::UnknownUnit,
                "{command:?}"
        );
    }

    assert_eq!(
        state.snapshot_bytes(),
        before,
            "a rejected command must not change the state"
    );
    assert!(state.unit_events.is_empty());
}

/// Every barrier/chat packet validates the team slot: `ALIEN` (4) and any
/// larger index are rejected as `Ignored` without touching the state.
#[test]
fn barrier_packets_reject_non_playing_team_slots() {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    let before = state.snapshot_bytes();

    for alien in [TeamId::ALIEN, TeamId(200)] {
        let commands = [
            command_finished_turn(alien),
            command_end_turn_barrier(alien, 1),
            Command::CrcBarrier(Packet45CrcBarrier {
                team: alien,
                next_turn_index: 1,
                crc16: 0x1234,
            }),
            Command::TurnBarrier(Packet00TurnBarrier {
                team: alien,
                counter: 1,
            }),
            Command::FrameBarrier(Packet01FrameBarrier {
                team: alien,
                counter: 1,
            }),
            Command::Chat(Packet18Chat {
                sender: alien,
                message: "hello".into(),
            }),
        ];

        for command in &commands {
            assert_eq!(
                apply_command(&mut state, command),
                ApplyOutcome::Ignored,
                    "team {alien:?}: {command:?}"
            );
        }
    }

    assert_eq!(state.snapshot_bytes(), before);
}

/// Packet 06 closes the turn only in turn-based play; in simultaneous mode
/// it only marks `finished_turn` and keeps the phase (`remote.cpp`).
#[test]
fn finished_turn_is_phase_dependent_on_the_play_mode() {
    let mut turn_based = GameState::new();
    turn_based.teams[0].team_type = TeamKind::Player;
    turn_based.teams[1].team_type = TeamKind::Remote;
    turn_based.play_mode = crate::enums::PlayMode::TurnBased;

    assert_eq!(
        apply_command(&mut turn_based, &command_finished_turn(TeamId::RED)),
        ApplyOutcome::Applied
    );
    assert!(turn_based.teams[0].finished_turn);
    assert_eq!(turn_based.game_state, crate::enums::GameStateType::EndTurn);
    assert!(
        !turn_based.are_teams_finished_turn(),
            "the remote team has not finished yet"
    );

    let mut simultaneous = GameState::new();
    simultaneous.teams[0].team_type = TeamKind::Player;
    simultaneous.play_mode = crate::enums::PlayMode::SimultaneousMoves;

    assert_eq!(
        apply_command(&mut simultaneous, &command_finished_turn(TeamId::RED)),
        ApplyOutcome::Applied
    );
    assert!(simultaneous.teams[0].finished_turn);
    assert_eq!(
        simultaneous.game_state,
        crate::enums::GameStateType::InGame,
            "simultaneous mode keeps playing"
    );
}

/// The seed delivery is ordered and deterministic: two seeds in one batch
/// end with the last one, and two identical runs stay bit-identical (D).
#[test]
fn seed_delivery_is_ordered_and_deterministic() {
    let (state, _id) = state_with_tank();

    fn run(base: &GameState) -> GameState {
        let mut state = base.clone();
        state.enqueue(Command::Seed(Packet13Seed { seed: 0x1111_1111 }));
        state.enqueue(Command::Seed(Packet13Seed { seed: 0x2222_2222 }));
        assert_eq!(state.apply_pending_commands(), 2);
        state
    }

    let first = run(&state);
    let second = run(&state);

    assert_eq!(first.snapshot_bytes(), second.snapshot_bytes());
    assert_eq!(
        crate::crc::state_digest(&first),
        crate::crc::state_digest(&second)
    );
    assert_eq!(first.rng, crate::rng::Randomizer::new(0x2222_2222));
    assert_ne!(first.rng.state(), state.rng.state());
}

/// `UnitEventEmergencyStop::Process` on a unit that no longer exists is a
/// no-op (the event stays a pure data record until the cycle).
#[test]
fn unit_event_for_a_missing_unit_is_a_no_op() {
    let mut state = GameState::new();
    let before = state.snapshot_bytes();

    process_unit_event(
        &mut state,
        &UnitEvent::EmergencyStop {
            unit: UnitId(0x7FFF),
        },
    );

    assert_eq!(state.snapshot_bytes(), before);
}
