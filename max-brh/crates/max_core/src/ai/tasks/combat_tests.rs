use super::*;
use crate::enums::{PlayMode, TeamClan};
use crate::geo::Point;
use crate::team::{TeamId, TeamKind, TeamUnits};
use crate::unit_values::{ATTRIB_ATTACK, ATTRIB_RANGE, ATTRIB_ROUNDS, ATTRIB_SPEED};
use crate::game::GameState;

fn state() -> GameState {
    let mut state = GameState::new();
    state.play_mode = PlayMode::TurnBased;
    state.teams[0].team_type = TeamKind::Computer;
    state.teams[1].team_type = TeamKind::Computer;
    state.team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units[1] = TeamUnits::with_clan(TeamClan::Random);
    state.ai.settings.opponent = Opponent::Average as u8;
    state.ai.players[0].init(state.ai.settings.opponent);
    state.ai.players[1].init(state.ai.settings.opponent);
    state
}

fn spawn(state: &mut GameState, unit_type: UnitType, team: TeamId, grid: Point) -> UnitId {
    state.spawn_unit(unit_type, team, grid, 0).expect("unit")
}

fn spot(state: &mut GameState, unit: UnitId, team: TeamId) {
    let mut info = state.unit(unit).expect("unit").clone();
    info.set_visible_to_team(team, true);
    state.write_unit(unit, info);
    crate::ai::ai_unit_spotted(state, unit, team);
}

#[test]
fn projected_damage_uses_attack_potential_and_rounds() {
    let world = GameState::new();
    let attacker = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
    let mut target = UnitInfo::new(UnitType::Tank, TeamId::GREEN, UnitId(2));
    target.hits = 7;

    let rounds = attacker
        .base_values
        .get_attribute(ATTRIB_ROUNDS);
    let damage = crate::combat::get_attack_damage(&attacker, &target, 0);

    assert_eq!(get_projected_damage(&world, &attacker, &target), damage * rounds);

    // An immobile target outranged by the attacker additionally yields hits - 1.
    target.base_values.set_attribute(ATTRIB_SPEED, 0);
    let target_range = target.base_values.get_attribute(ATTRIB_RANGE);
    let mut longer_reach = attacker.clone();
    longer_reach
        .base_values
        .set_attribute(ATTRIB_RANGE, target_range + 1);

    assert_eq!(
        get_projected_damage(&world, &longer_reach, &target),
        damage * rounds + i32::from(target.hits) - 1
    );
}

#[test]
fn target_flags_without_attacker_match_cpp_bits() {
    let world = GameState::new();

    let mut target = UnitInfo::new(UnitType::Tank, TeamId::GREEN, UnitId(2));
    target.ammo = 10;
    target.set_visible_to_team(TeamId::RED, true);

    let attack = target.base_values.get_attribute(ATTRIB_ATTACK);
    assert!(attack > 0 && attack <= 35);

    let expected = 0xFF - ((((attack / 8) * 8) as u32) + 0x80);
    assert_eq!(
        attack::get_target_flags(&world, None, &target, TeamId::RED),
        expected
    );

    // An unarmed, hidden target: no bits at all.
    target.ammo = 0;
    target.set_visible_to_team(TeamId::RED, false);
    assert_eq!(
        attack::get_target_flags(&world, None, &target, TeamId::RED),
        0xFF
    );
}

#[test]
fn attack_reserve_pools_then_hands_a_unit_to_a_kill_task() {
    let mut world = state();
    let tank = spawn(&mut world, UnitType::Tank, TeamId::RED, Point::new(5, 5));
    let target = spawn(&mut world, UnitType::Tank, TeamId::GREEN, Point::new(10, 10));
    spot(&mut world, target, TeamId::RED);

    world.with_task_manager(|tm, world| {
        let reserve_id = tm.append_task(
            world,
            super::super::super::Task::new(
                0,
                TeamId::RED,
                None,
                super::super::super::task::TASK_PRIORITY_ATTACK_RESERVE_POOL,
                TaskKind::AttackReserve {
                    units: Vec::new(),
                    site: Point::new(5, 5),
                    total_worth: 0,
                },
            ),
        );

        // No combat task yet: the unit stays in the pool.
        tm.task_add_unit(world, reserve_id, tank);

        match &tm.task(reserve_id).expect("reserve").kind {
            TaskKind::AttackReserve { units, .. } => assert!(units.contains(&tank)),
            _ => panic!("reserve"),
        }
        assert_eq!(tm.unit_task(tank), Some(reserve_id));

        let mut kill = super::super::KillUnitState::default();
        kill.spotted_unit = Some(0);
        kill.target_position = Point::new(10, 10);
        kill.target_type = Some(UnitType::Tank);
        kill.required_damage = 1;
        kill.hits = 20;
        kill.weight_table.add(UnitType::Tank, 1);

        let kill_id = tm.append_task(
            world,
            super::super::super::Task::new(
                0,
                TeamId::RED,
                None,
                super::super::super::task::TASK_PRIORITY_ATTACK_DEFAULT,
                TaskKind::KillUnit(Box::new(kill)),
            ),
        );

        // The reserve redirects the unit to the closest task that can use it.
        tm.task_add_unit(world, reserve_id, tank);

        assert_eq!(tm.unit_task(tank), Some(kill_id));

        match &tm.task(kill_id).expect("kill task").kind {
            TaskKind::KillUnit(state) => {
                assert_eq!(state.units, vec![tank]);
                assert_eq!(state.unit_damages.len(), 1);
                assert!(state.unit_damages[0].damage > 0);
            }
            _ => panic!("kill task"),
        }

        match &tm.task(reserve_id).expect("reserve").kind {
            TaskKind::AttackReserve { units, .. } => assert!(units.is_empty()),
            _ => panic!("reserve"),
        }
    });
}

#[test]
fn attack_readiness_advances_from_wait_to_attack() {
    let mut world = state();
    let tank = spawn(&mut world, UnitType::Tank, TeamId::RED, Point::new(5, 5));
    // A stationary building of the attacker's team makes the mobile target
    // "within reach" (`AiAttack_IsWithinReach`), otherwise the attack is not relevant.
    spawn(
        &mut world,
        UnitType::PowerStation,
        TeamId::RED,
        Point::new(8, 5),
    );

    let target = spawn(
        &mut world,
        UnitType::Tank,
        TeamId::GREEN,
        Point::new(7, 5),
    );
    {
        let mut info = world.unit(target).expect("target").clone();
        info.hits = 1;
        info.set_visible_to_team(TeamId::RED, true);
        world.write_unit(target, info);
    }
    crate::ai::ai_unit_spotted(&mut world, target, TeamId::RED);

    world.with_task_manager(|tm, world| {
        let attack_id = tm.append_task(
            world,
            super::super::super::Task::new(
                0,
                TeamId::RED,
                None,
                super::super::super::task::TASK_PRIORITY_ATTACK_DEFAULT,
                TaskKind::attack(TeamId::GREEN, Point::new(7, 5), Some(0)),
            ),
        );

        tm.task_begin_turn(world, attack_id);
        let kill_id = match &tm.task(attack_id).expect("attack").kind {
            TaskKind::Attack(state) => state.kill_unit_task.expect("kill task"),
            _ => panic!("attack"),
        };

        tm.task_begin_turn(world, kill_id);

        match &tm.task(kill_id).expect("kill task").kind {
            TaskKind::KillUnit(state) => assert!(state.units.contains(&tank)),
            _ => panic!("kill task"),
        }

        tm.task_end_turn(world, attack_id);

        let op_state = match &tm.task(attack_id).expect("attack").kind {
            TaskKind::Attack(state) => state.op_state,
            _ => panic!("attack"),
        };

        assert!(
            op_state >= attack_state::GATHER_FORCES,
                "readiness must leave WAIT, got {op_state}"
        );
        assert_eq!(
            op_state,
            attack_state::ATTACK,
                "with a visible target in range the attack must reach ATTACK"
        );
    });
}
