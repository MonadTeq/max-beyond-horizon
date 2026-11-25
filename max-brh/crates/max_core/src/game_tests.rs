use super::*;
use crate::enums::unit_flags;
use crate::geo::PathStep;
use crate::unit::{GroundPath, UnitPath};

fn tank_state() -> (GameState, UnitId, UnitId) {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = TeamKind::Player;
    state.team_units[0] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
    state.team_units[1] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
    let flags = unit_flags::MOBILE_LAND_UNIT | unit_flags::SELECTABLE;
    let first = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), flags)
        .expect("spawn");
    let second = state
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(20, 10), flags)
        .expect("spawn");
    (state, first, second)
}

#[test]
fn spawn_assigns_ids_and_lists_in_cpp_order() {
    let (state, first, second) = tank_state();
    assert_eq!(first.team(), TeamId::RED);
    assert_eq!(second.team(), TeamId::GREEN);
    assert_eq!(first.object_index(), 1);
    assert_eq!(second.object_index(), 2);
    assert_eq!(state.unit_count(), 2);
    assert_eq!(
        state.unit_list(UnitListKind::MobileLandSea),
        &[second, first],
        "AddToDrawList кладёт мобильные наземные в начало списка (PushFront)"
    );
    let unit = state.unit(first).expect("unit");
    assert_eq!(unit.unit_list, UnitListKind::MobileLandSea);
    assert_eq!(
        unit.hits,
        unit.base_values.get_attribute(ATTRIB_HITS) as u16
    );
    // `CTInfo::Reset` starts name counters at 1 (`ctinfo.hpp:49-51`).
    assert_eq!(unit.unit_id, 1);
    assert_eq!(state.teams[0].unit_counter(UnitType::Tank as u16), 2);
}

/// `MapHash::Add` is called with `mode = false`, which is the C++
/// `!mode` branch (`hash.cpp:170`): a new unit is pushed to the **front**
/// of its cell list.
#[test]
fn units_at_keeps_the_newest_unit_first_like_cpp() {
    let (mut state, first, second) = tank_state();
    let cell = Point::new(10, 10);
    assert_eq!(state.units_at(cell), &[first]);

    let flags = unit_flags::MOBILE_LAND_UNIT | unit_flags::SELECTABLE;
    let third = state
        .spawn_unit(UnitType::Tank, TeamId::RED, cell, flags)
        .expect("spawn");
    assert_eq!(
        state.units_at(cell),
        &[third, first],
        "the newest unit must be first (C++ PushFront)"
    );

    // A position change removes and re-adds the unit; it also lands at the
    // front of the destination cell (`write_unit` -> `Hash_MapHash.Add`).
    let mut moved = state.unit(second).expect("second").clone();
    moved.grid = cell;
    state.write_unit(second, moved);
    assert_eq!(state.units_at(cell), &[second, third, first]);
}

#[test]
fn destroy_removes_unit_from_every_structure() {
    let (mut state, first, _second) = tank_state();
    state.destroy_unit(first);
    assert!(!state.has_unit(first));
    assert_eq!(state.unit_count(), 1);
    assert!(!state
        .unit_list(UnitListKind::MobileLandSea)
        .contains(&first));
    assert_eq!(state.unit_ids_in_order().len(), 1);
}

#[test]
fn stationary_list_keeps_y_order() {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.team_units[0] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
    let flags = unit_flags::STATIONARY | unit_flags::BUILDING;
    let high = state
        .spawn_unit(
            UnitType::PowerGenerator,
            TeamId::RED,
            Point::new(5, 30),
            flags,
        )
        .unwrap();
    let low = state
        .spawn_unit(
            UnitType::PowerGenerator,
            TeamId::RED,
            Point::new(6, 10),
            flags,
        )
        .unwrap();
    let middle = state
        .spawn_unit(
            UnitType::PowerGenerator,
            TeamId::RED,
            Point::new(7, 20),
            flags,
        )
        .unwrap();
    assert_eq!(
        state.unit_list(UnitListKind::Stationary),
        &[low, middle, high]
    );
}

#[test]
fn score_graph_shifts_like_memmove() {
    let mut state = GameState::new();
    state.teams[0].score_graph = [7; SCORE_GRAPH_LEN];
    state.teams[0].team_points = 42;
    state.update_score_graph();
    assert_eq!(state.teams[0].score_graph[0], 7);
    assert_eq!(state.teams[0].score_graph[SCORE_GRAPH_LEN - 1], 42);
    for index in 0..SCORE_GRAPH_LEN - 2 {
        assert_eq!(state.teams[0].score_graph[index], 7);
    }
    assert_eq!(state.teams[0].score_graph[SCORE_GRAPH_LEN - 2], 7);
}

#[test]
fn finish_turn_advances_counter_and_resets_flags() {
    let (mut state, _red, _green) = tank_state();
    // P7: end of turn first checks victory/defeat
    // (`menu_check_end_game_conditions`); a live team + the limit not reached →
    // the game continues. The network branch is disabled (local semantics).
    state.network_game = false;
    state.victory_type = VictoryType::Duration;
    state.victory_limit = 50;
    state.teams[0].finished_turn = true;
    state.game_state = GameStateType::EndTurn;
    assert!(!state.finish_turn());
    assert_eq!(state.turn_counter, 1);
    assert!(!state.teams[0].finished_turn);
    assert_eq!(state.game_state, GameStateType::InGame);
}

#[test]
fn finish_turn_stops_when_victory_condition_met() {
    let (mut state, _red, _green) = tank_state();
    state.network_game = false;
    // `VICTORY_TYPE_DURATION` with a zero limit: `limit <= turn_counter` → over.
    state.victory_type = VictoryType::Duration;
    state.victory_limit = 0;
    state.game_state = GameStateType::EndTurn;
    assert!(state.finish_turn());
    assert_eq!(state.turn_counter, 0);
}

#[test]
fn pending_attack_head_is_settled_like_cpp() {
    let (mut state, first, _second) = tank_state();
    // An attacker with zero hits is simply removed from the queue.
    {
        let unit = state.unit_mut(first).unwrap();
        unit.hits = 0;
        unit.orders = UnitOrder::Fire;
        unit.state = UnitOrderState::AttackPending;
    }
    state.pending_attacks.push(first);
    state.process_orders();
    assert!(state.pending_attacks.is_empty());

    // An attacker whose order is not FIRE — the attack is cancelled.
    {
        let unit = state.unit_mut(first).unwrap();
        unit.hits = 10;
        unit.orders = UnitOrder::Await;
        unit.prior_orders = UnitOrder::Await;
    }
    state.pending_attacks.push(first);
    state.process_orders();
    assert!(state.pending_attacks.is_empty());
}

#[test]
fn process_orders_visits_five_lists_in_canonical_order() {
    use crate::enums::unit_flags::{
        GROUND_COVER, MISSILE_UNIT, MOBILE_AIR_UNIT, MOBILE_LAND_UNIT, STATIONARY,
    };

    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.team_units[0] = TeamUnits::with_clan(crate::enums::TeamClan::Random);

    // One unit in each of the five lists, in the canary order
    // (DESIGN §5.2: GroundCover → MobileLandSea → Stationary → MobileAir →
    // Particle).
    let ground_cover = state
        .spawn_unit(
            UnitType::SmallSlab,
            TeamId::RED,
            Point::new(1, 1),
            STATIONARY | GROUND_COVER,
        )
        .unwrap();
    let mobile_land = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            Point::new(2, 2),
            MOBILE_LAND_UNIT,
        )
        .unwrap();
    let stationary = state
        .spawn_unit(
            UnitType::PowerGenerator,
            TeamId::RED,
            Point::new(3, 3),
            STATIONARY,
        )
        .unwrap();
    let mobile_air = state
        .spawn_unit(
            UnitType::Fighter,
            TeamId::RED,
            Point::new(4, 4),
            MOBILE_AIR_UNIT,
        )
        .unwrap();
    let particle = state
        .spawn_unit(
            UnitType::Rocket,
            TeamId::RED,
            Point::new(5, 5),
            MISSILE_UNIT,
        )
        .unwrap();

    let expected = [ground_cover, mobile_land, stationary, mobile_air, particle];
    assert_eq!(state.unit_ids_in_order(), expected);

    // `ORDER_FIRE` in INIT state adds the unit in one pass to
    // `PendingAttacks` and `LiveAttacks` — insertion order is the order
    // of list traversal.
    for unit in expected {
        state.set_order(unit, UnitOrder::Fire, UnitOrderState::Init);
    }
    state.tick();

    assert_eq!(state.live_attacks.len(), expected.len());
    for (attack, unit) in state.live_attacks.iter().zip(expected.iter()) {
        assert_eq!(
            state.unit(*unit).unwrap().live_attack,
            attack.id,
            "живая атака юнита {unit:?} создана не в каноническом порядке"
        );
    }
    // A missile (`MISSILE_UNIT`) at the head of `UnitsManager_ProcessOrder` sets
    // `OrdersPending`, so the attack queue is not processed this tick —
    // as in C++.
    assert!(state.orders_pending);
    assert_eq!(state.pending_attacks, expected.to_vec());
    for unit in expected {
        assert_eq!(
            state.unit(unit).unwrap().state,
            UnitOrderState::AttackPending
        );
    }
}

#[test]
fn scripted_combat_replay_is_deterministic() {
    use crate::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE, UPGRADABLE};

    fn run() -> (Vec<u8>, u16, u16, u8) {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.teams[1].team_type = TeamKind::Player;
        state.team_units[0] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
        state.team_units[1] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
        state.rng.set_seed(0xDEAD_BEEF);

        let red = state
            .spawn_unit(
                UnitType::Tank,
                TeamId::RED,
                Point::new(10, 10),
                MOBILE_LAND_UNIT | SELECTABLE | UPGRADABLE,
            )
            .unwrap();
        let green = state
            .spawn_unit(
                UnitType::Tank,
                TeamId::GREEN,
                Point::new(13, 10),
                MOBILE_LAND_UNIT | SELECTABLE | UPGRADABLE,
            )
            .unwrap();

        // A series of shots: each FIRE order is one shot (after
        // the shot `RestoreOrders` returns AWAIT); the start of the turn
        // restores `shots` (`UpdateProduction`).
        //
        // The next shot waits in `ORDER_STATE_ATTACK_PENDING` until the hit effect
        // (`HITEXPLD`, `UnitsManager_StartExplosion` with `hits > 0`) finishes its
        // animation: with `unit_order::EXPLOSION_FRAME_STEPS` that is
        // `5 × (5 − 1) + 1 = 21` simulation steps (~0.35 s — the oracle's
        // `2 × (5 − 1) + 1` order ticks × 42 ms). The window per shot therefore
        // covers a full cycle: one turn step + recoil + the hit animation.
        for _ in 0..8 {
            state.manage_economy(TeamId::RED);
            let mut data = state.order_data(red);
            data.fire_on = Point::new(13, 10);
            data.move_to = Point::new(13, 10);
            state.set_order_data(red, data);
            state.set_order(red, UnitOrder::Fire, UnitOrderState::Init);
            for _ in 0..40 {
                state.tick();
            }
            if !state.has_unit(green) || state.unit(green).map(|unit| unit.hits).unwrap_or(0) == 0 {
                break;
            }
        }
        for _ in 0..2 {
            state.tick();
        }

        let green_hits = state.unit(green).map(|unit| unit.hits).unwrap_or(0);
        let red_ammo = state.unit(red).map(|unit| unit.ammo).unwrap_or(0);
        let red_shots = state.unit(red).map(|unit| unit.shots).unwrap_or(0);
        (
            state.snapshot_bytes(),
            green_hits,
            u16::from(red_ammo),
            red_shots,
        )
    }

    let first = run();
    let second = run();
    assert_eq!(first.0, second.0, "боевой реплей недетерминирован");
    assert_eq!((first.1, first.2, first.3), (second.1, second.2, second.3));
    // 4 shots of 6 damage (24 hp) destroy the target; the behavior
    // is pinned so that a formula regression is visible.
    assert_eq!(first.1, 0, "цель должна быть уничтожена");
    assert_eq!(first.2, 10, "израсходовано 4 патрона из 14");
}

#[test]
fn scripted_replay_is_deterministic() {
    use crate::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};

    fn run() -> Vec<u8> {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.teams[1].team_type = TeamKind::Computer;
        state.team_units[0] = TeamUnits::with_clan(crate::enums::TeamClan::Random);
        state.team_units[1] = TeamUnits::with_clan(crate::enums::TeamClan::Random);

        let red = state
            .spawn_unit(
                UnitType::Tank,
                TeamId::RED,
                Point::new(10, 10),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .unwrap();
        let green = state
            .spawn_unit(
                UnitType::Tank,
                TeamId::GREEN,
                Point::new(20, 10),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .unwrap();

        // 13: seed; 08: order with targets (P3 payload); 38: ready path;
        // 50: emergency stop; 41: movement finished; 06/52: end of turn.
        state.enqueue(Command::Seed(crate::input::Packet13Seed {
            seed: 0xDEAD_BEEF,
        }));
        state.enqueue(Command::Order(crate::input::Packet08Order {
            unit: red,
            payload: crate::input::OrderPayload::P3(crate::input::PayloadP3 {
                p2: crate::input::PayloadP2 {
                    p1: crate::events::payload_p1_for(
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
        }));
        for _ in 0..6 {
            state.tick();
        }
        state.enqueue(crate::events::command_move_linear(
            green,
            Point::new(20, 10),
            Point::new(20, 13),
            UnitOrder::Move,
            UnitOrderState::InProgress,
        ));
        state.tick();
        state.enqueue(crate::events::command_move_linear(
            red,
            Point::new(14, 10),
            Point::new(18, 10),
            UnitOrder::Move,
            UnitOrderState::InProgress,
        ));
        state.enqueue(Command::Stop(crate::input::Packet50StopUnit {
            unit: green,
        }));
        state.enqueue(Command::MoveFinished(
            crate::input::Packet41MoveFinished::port_form(red, true),
        ));
        for _ in 0..5 {
            state.tick();
        }
        state.enqueue(crate::events::command_finished_turn(TeamId::RED));
        state.enqueue(crate::events::command_finished_turn(TeamId::GREEN));
        state.enqueue(crate::events::command_end_turn_barrier(TeamId::RED, 1));
        state.tick();
        state.finish_turn();
        for _ in 0..3 {
            state.tick();
        }
        state.snapshot_bytes()
    }

    let first = run();
    let second = run();
    assert_eq!(first, second, "два прогона одного скрипта разошлись");
    assert!(!first.is_empty());
}

#[test]
fn end_turn_restores_speed_and_updates_production() {
    let (mut state, id, _second) = tank_state();
    // Spend speed and shots.
    {
        let unit = state.unit_mut(id).unwrap();
        unit.speed = 0;
        unit.shots = 0;
        unit.moved = 5;
    }
    let team = state.unit(id).unwrap().team;
    state.manage_economy(team);
    let unit = state.unit(id).unwrap();
    assert_eq!(
        unit.speed,
        unit.base_values.get_attribute(ATTRIB_SPEED) as u16,
        "UpdateProduction возвращает скорость на начало хода"
    );
    assert_eq!(
        unit.shots,
        unit.base_values.get_attribute(ATTRIB_ROUNDS) as u8
    );
}

#[test]
fn barriers_require_agreement_of_playing_teams() {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = TeamKind::Remote;
    state.teams[2].team_type = TeamKind::None;
    state.teams[3].team_type = TeamKind::Eliminated;

    // The barrier is not there yet: default values are equal (0), but this is
    // the "before turn start" agreement — as in C++, counters start at 0.
    assert_eq!(state.end_turn_barrier(), Some(0));

    state.turn_indices[0] = 3;
    assert_eq!(
        state.end_turn_barrier(),
        None,
        "команды разошлись по счётчику"
    );
    state.turn_indices[1] = 3;
    assert_eq!(state.end_turn_barrier(), Some(3));
    // ALIEN/empty slots do not participate in the barrier.
    state.turn_indices[4] = 9;
    assert_eq!(state.end_turn_barrier(), Some(3));

    state.next_turn_indices[0] = 5;
    state.next_turn_indices[1] = 6;
    assert_eq!(state.crc_barrier(), None);
    state.next_turn_indices[1] = 5;
    assert_eq!(state.crc_barrier(), Some(5));
}

#[test]
fn begin_turn_barrier_tracks_frame_sync_counter() {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = TeamKind::Remote;
    state.teams[2].team_type = TeamKind::None;

    assert_eq!(state.begin_turn_barrier(), Some(0));
    state.frame_sync_counter[0] = 1;
    assert_eq!(state.begin_turn_barrier(), None, "пир ещё не вошёл в раунд");
    state.frame_sync_counter[1] = 1;
    assert_eq!(state.begin_turn_barrier(), Some(1));

    // The 54 barrier completes only on a full (`is_last`) state.
    assert_eq!(state.state_sync_index(TeamId::GREEN), 0);
    state.state_sync_indices[1] = 4;
    assert_eq!(state.state_sync_index(TeamId::GREEN), 4);
}

#[test]
fn snapshot_is_stable_across_clones() {
    let (state, first, _second) = tank_state();

    // Two independent runs of the same command sequence
    // give a byte-identical state.
    let mut first_run = state.clone();
    let mut second_run = state.clone();
    for _ in 0..5 {
        first_run.enqueue(Command::Stop(crate::input::Packet50StopUnit {
            unit: first,
        }));
        second_run.enqueue(Command::Stop(crate::input::Packet50StopUnit {
            unit: first,
        }));
        first_run.tick();
        second_run.tick();
    }
    assert_eq!(first_run.snapshot_bytes(), second_run.snapshot_bytes());

    // An order change changes the digest.
    let before = first_run.snapshot_bytes();
    first_run.set_order(first, UnitOrder::Sentry, UnitOrderState::ExecutingOrder);
    assert_ne!(before, first_run.snapshot_bytes());
}

#[test]
fn path_kind_is_encoded_in_snapshot() {
    let (mut state, first, _second) = tank_state();
    let before = state.snapshot_bytes();
    {
        let unit = state.unit_mut(first).unwrap();
        unit.path = Some(UnitPath::Ground(GroundPath {
            end_x: 12,
            end_y: 10,
            step_index: 0,
            steps: vec![PathStep::new(1, 0), PathStep::new(1, 0)],
        }));
    }
    assert_ne!(before, state.snapshot_bytes());
}

/// `ResourceManager_InitClanUnitValues` (`resource_manager.cpp:1275-1290`):
/// the derelict team is always `TEAM_CLAN_THE_CHOSEN`, independent of the
/// menu clan settings.
#[test]
fn derelict_team_uses_the_chosen_clan() {
    let state = GameState::with_setup(
        crate::map::Map::new(),
        [
            TeamKind::Player,
            TeamKind::None,
            TeamKind::None,
            TeamKind::None,
        ],
    );
    let alien = PLAYER_TEAM_COUNT;
    assert_eq!(
        state.teams[alien].team_clan,
        crate::enums::TeamClan::TheChosen
    );
    let expected = crate::unit_values::team_base_values(crate::enums::TeamClan::TheChosen);
    assert_eq!(state.team_units[alien].base_values, expected);
    assert_eq!(state.team_units[alien].current_values, expected);
    assert!(state.team_units[alien].complexes().is_empty());

    // The Chosen bonuses touch air units only: alien debris types keep the
    // `SC_ATTRI` values (the same observable result as in C++).
    assert_eq!(
        state.team_units[alien].base_values[UnitType::AlienTank as usize],
        crate::unit_values::base_unit_values(UnitType::AlienTank)
    );
}

// -----------------------------------------------------------------------
// Phase 1: turn state machine (U)
// -----------------------------------------------------------------------

/// `GameManager_TeamTurnFinish` closes the turn only from `GAME_STATE_END_TURN`.
/// Any other phase is a no-op: the counter and the phase stay untouched.
#[test]
fn finish_turn_is_guarded_by_the_end_turn_phase() {
    let (mut state, _red, _green) = tank_state();
    state.network_game = false;
    state.victory_limit = 50;

    for phase in [
        GameStateType::InGame,
        GameStateType::TurnActive,
        GameStateType::DeployingUnits,
        GameStateType::SiteSelect,
    ] {
        state.game_state = phase;
        assert!(
            !state.finish_turn(),
            "phase {phase:?} must not close the turn"
        );
        assert_eq!(state.turn_counter, 0, "phase {phase:?}");
        assert_eq!(state.game_state, phase, "phase {phase:?} is left as is");
    }

    state.game_state = GameStateType::EndTurn;
    assert!(!state.finish_turn(), "no victory condition is met");
    assert_eq!(state.turn_counter, 1);
    assert_eq!(state.game_state, GameStateType::InGame);
}

/// Boundary: a game with no playing teams has no barrier agreement, yet
/// `menu_check_end_game_conditions` finds nothing to end and the turn
/// counter still advances (0 participants is not "everyone is out").
#[test]
fn empty_game_has_no_barrier_and_still_advances_the_turn() {
    let mut state = GameState::new();

    assert_eq!(
        state.end_turn_barrier(),
        None,
        "no playing teams — no agreement"
    );
    assert_eq!(state.begin_turn_barrier(), None);
    assert_eq!(state.crc_barrier(), None);
    assert!(state.are_teams_finished_turn());

    state.game_state = GameStateType::EndTurn;
    assert!(!state.finish_turn(), "empty game has no end condition");
    assert_eq!(state.turn_counter, 1);
    assert_eq!(state.game_state, GameStateType::InGame);
}

/// `GameManager_IsActiveTurn` (`game_manager.cpp:8296`): the active team in
/// turn-based play, everyone in simultaneous play, and always a remote team.
#[test]
fn is_active_turn_follows_the_play_mode() {
    let (mut state, _red, _green) = tank_state();
    state.active_turn_team = TeamId::RED;
    state.play_mode = PlayMode::TurnBased;
    assert!(state.is_active_turn(TeamId::RED));
    assert!(!state.is_active_turn(TeamId::GREEN));

    state.teams[TeamId::GREEN.0 as usize].team_type = TeamKind::Remote;
    assert!(
        state.is_active_turn(TeamId::GREEN),
        "a remote team always processes its input"
    );

    state.play_mode = PlayMode::SimultaneousMoves;
    assert!(state.is_active_turn(TeamId::BLUE));
    assert!(state.is_active_turn(TeamId::GRAY));

    state.play_mode = PlayMode::Unknown;
    assert!(state.is_active_turn(TeamId::GRAY));
}

/// The barrier needs the agreement of all four playing teams; `ALIEN` and
/// empty/eliminated slots never take part (`Remote_ServerWaitBarrier`).
#[test]
fn barrier_requires_all_four_playing_teams_to_agree() {
    let mut state = GameState::new();
    for team in 0..PLAYER_TEAM_COUNT {
        state.teams[team].team_type = TeamKind::Player;
        state.turn_indices[team] = 9;
    }
    assert_eq!(state.end_turn_barrier(), Some(9));

    state.turn_indices[PLAYER_TEAM_COUNT - 1] = 8;
    assert_eq!(state.end_turn_barrier(), None, "one team diverged");

    state.turn_indices[PLAYER_TEAM_COUNT] = 1;
    assert_eq!(
        state.end_turn_barrier(),
        None,
        "ALIEN does not repair a disagreement"
    );

    state.turn_indices[PLAYER_TEAM_COUNT - 1] = 9;
    assert_eq!(state.end_turn_barrier(), Some(9));

    // An eliminated team drops out of the agreement (as in C++).
    state.teams[PLAYER_TEAM_COUNT - 1].team_type = TeamKind::Eliminated;
    state.turn_indices[PLAYER_TEAM_COUNT - 1] = 0;
    assert_eq!(state.end_turn_barrier(), Some(9));
}

/// Commands are applied strictly in receive order: conflicting orders produce
/// the last delivered one, and the reverse delivery produces the other state.
/// Both runs are bit-identical to their own repetition (D).
#[test]
fn command_delivery_order_decides_the_final_order() {
    fn order_command(id: UnitId, order: UnitOrder) -> Command {
        Command::Order(crate::input::Packet08Order {
            unit: id,
            payload: crate::input::OrderPayload::P1(crate::events::payload_p1_for(
                order,
                UnitOrderState::ExecutingOrder,
                UnitOrder::Await,
                UnitOrderState::ExecutingOrder,
            )),
        })
    }

    fn run(base: &GameState, id: UnitId, first: UnitOrder, second: UnitOrder) -> GameState {
        let mut state = base.clone();
        state.enqueue(order_command(id, first));
        state.enqueue(order_command(id, second));
        assert_eq!(state.apply_pending_commands(), 2);
        state
    }

    let (state, id, _second) = tank_state();
    let sentry = UnitOrder::Sentry;
    let await_order = UnitOrder::Await;

    let forward = run(&state, id, sentry, await_order);
    assert_eq!(
        forward.unit(id).expect("unit").orders,
        await_order,
        "the last delivered order wins"
    );

    let backward = run(&state, id, await_order, sentry);
    assert_eq!(backward.unit(id).expect("unit").orders, sentry);
    assert_ne!(
        forward.snapshot_bytes(),
        backward.snapshot_bytes(),
        "delivery order is part of the state"
    );

    // Repetition of the same delivery order stays bit-identical.
    let repeated = run(&state, id, sentry, await_order);
    assert_eq!(repeated.snapshot_bytes(), forward.snapshot_bytes());
    assert_eq!(
        crate::crc::state_digest(&repeated),
        crate::crc::state_digest(&forward)
    );
}

/// Phase, active team and turn counter are simulated fields: they enter the
/// canonical digest but not the packet-45 unit-list CRC (D).
#[test]
fn phase_team_and_turn_counter_enter_the_digest() {
    let (mut state, _red, _green) = tank_state();
    state.network_game = false;

    let baseline = state.snapshot_bytes();
    let baseline_crc = crate::crc::state_digest(&state);
    let unit_crc = crate::crc::state_checksum(&state);

    state.active_turn_team = TeamId::GREEN;
    let after_team = state.snapshot_bytes();
    assert_ne!(baseline, after_team, "active team is digested");
    assert_ne!(baseline_crc, crate::crc::state_digest(&state));

    state.game_state = GameStateType::EndTurn;
    assert_ne!(after_team, state.snapshot_bytes(), "phase is digested");

    state.game_state = GameStateType::InGame;
    state.turn_counter += 1;
    assert_ne!(baseline, state.snapshot_bytes(), "turn is digested");

    // Packet 45 covers only the four unit lists (no phase/turn fields).
    assert_eq!(
        crate::crc::state_checksum(&state),
        unit_crc,
        "unit-list CRC must not depend on the phase"
    );
}

/// `UnitId` carries a 13-bit object index; a long run of spawns never
/// leaves that space and keeps the C++ `UnitsManager_UnitIdCounter`
/// monotonic (boundary/MAX).
#[test]
fn unit_ids_stay_inside_the_thirteen_bit_index_space() {
    let (mut state, _red, _green) = tank_state();
    let before = state.unit_id_counter;
    let flags = unit_flags::MOBILE_LAND_UNIT | unit_flags::SELECTABLE;
    let mut seen = std::collections::BTreeSet::new();

    for index in 0..64i16 {
        let grid = Point::new(2 + index % 60, 2 + index / 60);
        let id = state
            .spawn_unit(UnitType::Tank, TeamId::RED, grid, flags)
            .expect("spawn");
        assert!(
            id.object_index() < 0x2000,
            "object index {} escapes the 13-bit space",
            id.object_index()
        );
        assert!(seen.insert(id), "duplicate unit id {id:?}");
    }

    assert_eq!(state.unit_id_counter, before + 64);
    assert_eq!(state.unit_count(), 2 + 64);
}
