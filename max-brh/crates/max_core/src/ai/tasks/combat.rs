//! Combat: `TaskAttack`, `TaskKillUnit`, `TaskFrontalAttack`, `TaskWaitToAttack`,
//! `TaskSupportAttack`, `TaskAttackReserve`, `TaskDefenseReserve`,
//! `TaskEscort`, `TaskRetreat`.
//!
//! Oracle: `taskattack.cpp/.hpp`, `taskkillunit.cpp/.hpp`,
//! `taskfrontalattack.cpp/.hpp`, `taskwaittoattack.cpp/.hpp`,
//! `tasksupportattack.cpp/.hpp`, `taskattackreserve.cpp/.hpp`,
//! `taskdefensereserve.cpp/.hpp`, `taskescort.cpp/.hpp`,
//! `taskretreat.cpp/.hpp`, `aiattack.cpp` (attack decisions — the module
//! [`crate::ai::attack`]).
//!
//! # What is ported
//!
//! * `TaskAttack`: `target_team`/`op_state`/`primary_targets`/
//!   `secondary_targets`, `Init`, `BeginTurn`, `EndTurn`, `AddUnit`,
//!   `IsUnitUsable`, `Execute`, `RemoveSelf`, `RemoveUnit`, `ChildComplete`,
//!   `ExchangeOperator` (structurally), `GetCautionLevel`.
//! * `TaskKillUnit`: `spotted_unit`, `units`, `unit_damages`,
//!   `required_damage`, `weight_table`, `seek_target`, `recruit_turn`;
//!   `GetProjectedDamage`, `IsUnitSuitable`, `BeginTurn`, `EndTurn`,
//!   `Execute`, `EventUnitDestroyed`, `EventEnemyUnitSpotted`,
//!   `HoldRecruitsThisTurn`.
//! * `TaskWaitToAttack`: `Init`, `EndTurn`, `CheckReactions`, `IsThinking`.
//! * `TaskRetreat`: searching a safe cell by the damage map
//!   (`Ai_IsDangerousLocation`/`GetDamagePotentialMap`) and `TaskMove` to it.
//! * `TaskFrontalAttack`, `TaskSupportAttack`, `TaskAttackReserve`,
//!   `TaskDefenseReserve`, `TaskEscort` — life cycle and orders; their internal
//!   heuristics (transport maps, defense sectors, `TransporterMap`) are reduced
//!   to deterministic site/unit selection (see notes in the functions).

use crate::enums::{unit_flags, CautionLevel, Opponent, UnitOrder, UnitOrderState, UnitType};
use crate::geo::Point;
use crate::team::TeamId;
use crate::unit::{UnitId, UnitInfo, UnitListKind};

use super::super::attack;
use super::super::task::{
    TaskHeader, TASK_PRIORITY_ATTACK_MOBILE, TASK_PRIORITY_ATTACK_PRIORITY_TARGET,
    TASK_PRIORITY_EMERGENCY, TASK_PRIORITY_ESCORT, TASK_PRIORITY_FOLLOW_ATTACK,
    TASK_PRIORITY_FOLLOW_DEFENSE, TASK_PRIORITY_REPAIR, TASK_TYPE_OBTAIN_UNITS,
};
use super::{attack_state, TaskCtx, TaskKind, UnitDamage};

// ---------------------------------------------------------------------------
// Common helpers
// ---------------------------------------------------------------------------

/// The `TaskKillUnit` target by `SpottedUnit` index (`AiPlayer::spotted_units`).
pub(crate) fn spotted_target(
    ctx: &TaskCtx<'_>,
    team: TeamId,
    index: u32,
) -> Option<(UnitId, Point, UnitType)> {
    let player = &ctx.world.ai.players[team.0 as usize];
    let spotted = player.spotted_units.get(index as usize)?;
    let unit_id = spotted.unit?;
    let info = ctx.unit(unit_id)?;

    Some((unit_id, spotted.last_position, info.unit_type))
}

/// The `SpottedUnit` target without `TaskCtx` (for the `aiattack.cpp` layer).
pub fn spotted_target_for_ai(
    world: &crate::game::GameState,
    team: TeamId,
    index: u32,
) -> Option<(UnitId, Point, UnitType)> {
    let player = &world.ai.players[team.0 as usize];
    let spotted = player.spotted_units.get(index as usize)?;
    let unit_id = spotted.unit?;
    let info = world.unit(unit_id)?;

    Some((unit_id, spotted.last_position, info.unit_type))
}

/// `TaskKillUnit::GetProjectedDamage` (static) — the group's total damage.
pub(crate) fn kill_unit_total_projected_damage(state: &super::KillUnitState) -> i32 {
    state.unit_damages.iter().map(|entry| entry.damage).sum()
}

/// The live target of a kill task (`TaskKillUnit::GetUnitSpotted`).
fn kill_unit_target(
    ctx: &TaskCtx<'_>,
    team: TeamId,
    state: &super::KillUnitState,
) -> Option<(UnitId, UnitInfo)> {
    let (unit_id, _, _) = spotted_target(ctx, team, state.spotted_unit?)?;
    let info = ctx.unit(unit_id)?;

    Some((unit_id, info))
}

/// `TaskKillUnit::GetProjectedDamage(attacker, target)` (static) — expected damage.
///
/// `AiAttack_GetAttackPotential` already accounts for sabotage, so the result is
/// multiplied by the attacker's rounds; a target that cannot answer (immobile and
/// outranged, or a type the attacker cannot be hit by) additionally yields
/// `hits - 1`, as in C++.
pub fn get_projected_damage(
    world: &crate::game::GameState,
    attacker: &UnitInfo,
    target: &UnitInfo,
) -> i32 {
    let rounds = attacker
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_ROUNDS);

    let mut damage_potential =
        attack::get_attack_potential(world, attacker, target) * rounds;

    if target
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_ATTACK)
        > 0
    {
        let target_speed = target
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_SPEED);
        let attacker_range = attacker
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_RANGE);
        let target_range = target
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_RANGE);

        if (target_speed == 0 && attacker_range > target_range)
            || !crate::access::is_valid_attack_target_type(
                target.unit_type,
                attacker.unit_type,
            )
        {
            damage_potential += i32::from(target.hits) - 1;
        }
    }

    damage_potential
}

/// `TaskKillUnit::IsUnitSuitable` — the unit can join the kill task.
fn is_unit_suitable(
    ctx: &TaskCtx<'_>,
    team: TeamId,
    state: &super::KillUnitState,
    unit: UnitId,
) -> bool {
    let Some(info) = ctx.unit(unit) else {
        return false;
    };

    if info.team != team || info.hits == 0 || state.spotted_unit.is_none() {
        return false;
    }

    if ctx.world.turn_counter < state.recruit_turn {
        return false;
    }

    let rounds = info
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_ROUNDS);

    if rounds <= 0 || i32::from(info.ammo) < rounds {
        return false;
    }

    if state.managed_unit == Some(unit) {
        return false;
    }

    if state.weight_table.get_weight(info.unit_type) == 0 {
        return false;
    }

    i32::from(state.hits) > kill_unit_total_projected_damage(state)
}

/// `TaskAttack::GetUnitValue` (`taskattack.cpp:2205`) — the build cost of the
/// units assigned to the attack's kill tasks.
fn attack_unit_value(
    tm: &super::super::task_manager::TaskManager,
    world: &crate::game::GameState,
    attack_state: &super::AttackState,
) -> i32 {
    let mut result = 0;

    for task_id in &attack_state.secondary_targets {
        let Some(task) = tm.task(*task_id) else {
            continue;
        };

        if let TaskKind::KillUnit(kill) = &task.kind {
            for unit_id in &kill.units {
                if let Some(unit) = world.unit(*unit_id) {
                    result += super::super::strategy::normal_rate_build_cost(
                        world,
                        unit.team,
                        unit.unit_type,
                    );
                }
            }
        }
    }

    result
}

/// `TaskKillUnit::IsAttackShareLeft` — the attack draws units up to its share
/// of the team's military value.
fn is_attack_share_left(ctx: &TaskCtx<'_>, team: TeamId, header: TaskHeader) -> bool {
    let Some(parent_id) = header.parent else {
        return true;
    };

    let Some(TaskKind::Attack(attack_state)) = ctx.tm.task(parent_id).map(|task| &task.kind)
    else {
        return true;
    };

    let share = ctx.world.ai.players[team.0 as usize].attack_value_share;

    attack_unit_value(ctx.tm, ctx.world, attack_state) < share
}

/// `TaskAttack::GetDefendersFire` (`taskattack.cpp:2217`) — the fire of the
/// armed enemies covering the current target, counted once per target and turn.
fn attack_get_defenders_fire(
    ctx: &mut TaskCtx<'_>,
    team: TeamId,
    kill_task_id: u32,
    kill_state: &super::KillUnitState,
) -> i32 {
    let Some((target_id, target)) = kill_unit_target(ctx, team, kill_state) else {
        return 0;
    };

    let Some(parent_id) = ctx.tm.task(kill_task_id).and_then(|task| task.parent) else {
        return 0;
    };

    let turn = ctx.world.turn_counter;

    let needs_update = {
        let Some(TaskKind::Attack(attack_state)) =
            ctx.tm.task(parent_id).map(|task| &task.kind)
        else {
            return 0;
        };

        attack_state.fire_target != Some(target_id) || attack_state.fire_turn != turn
    };

    if needs_update {
        let position = kill_state.target_position;
        let mut defenders_fire = 0;

        let player = &ctx.world.ai.players[team.0 as usize];
        for spotted in &player.spotted_units {
            let Some(unit_id) = spotted.unit else {
                continue;
            };
            if unit_id == target_id {
                continue;
            }
            let Some(unit) = ctx.world.unit(unit_id) else {
                continue;
            };

            if unit.hits == 0
                || unit.ammo == 0
                || !player.is_enemy_team(ctx.world, unit.team)
            {
                continue;
            }

            let range = crate::combat::get_attack_range(unit);
            if range > 0
                && crate::access::squared_distance(unit.grid, position) <= range * range
            {
                defenders_fire += unit
                    .base_values
                    .get_attribute(crate::unit_values::ATTRIB_ATTACK)
                    * unit
                        .base_values
                        .get_attribute(crate::unit_values::ATTRIB_ROUNDS);
            }
        }

        let _ = target;

        if let Some(TaskKind::Attack(attack_state)) =
            ctx.tm.task_mut(parent_id).map(|task| &mut task.kind)
        {
            attack_state.fire_target = Some(target_id);
            attack_state.fire_turn = turn;
            attack_state.defenders_fire = defenders_fire;
        }
    }

    match ctx.tm.task(parent_id).map(|task| &task.kind) {
        Some(TaskKind::Attack(attack_state)) => attack_state.defenders_fire,
        _ => 0,
    }
}

/// `TaskKillUnit::UpdateDemand` (`taskkillunit.cpp:511`) — recompute the required
/// damage and the group's target damage (`hits`).
///
/// Unlike the earlier reduced version this does **not** recompute the unit
/// contributions: `unit_damages` keeps the damage each unit was counted with when
/// it joined (`TaskKillUnit::AddUnit`), exactly like C++.
fn kill_unit_update_demand(
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    state: &mut super::KillUnitState,
) {

    let Some((target_id, target)) = kill_unit_target(ctx, header.team, state) else {
        state.required_damage = 1;
        state.hits = 0;
        return;
    };

    let target_ammo = target
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_AMMO);

    let (required_damage, wanted) =
        if target_ammo > 0 && target.team != TeamId::ALIEN {
            (target.hits, 2 * i32::from(target.hits))
        } else {
            (1, i32::from(target.hits))
        };

    // The defenders fire once per attack; only the attack's current target
    // recruits for it.
    let is_current_target = match ctx.tm.task(header.id).and_then(|task| task.parent) {
        Some(parent_id) => match ctx.tm.task(parent_id).map(|task| &task.kind) {
            Some(TaskKind::Attack(attack_state)) => attack_state.kill_unit_task == Some(header.id),
            _ => false,
        },
        None => false,
    };

    let wants = if is_current_target {
        wanted + attack_get_defenders_fire(ctx, header.team, header.id, state)
    } else {
        wanted
    };

    state.required_damage = required_damage;
    state.hits = ((wants * 5) / 4).clamp(0, i32::from(i16::MAX)) as u16;
    state.target_position = target.grid;
    state.target_type = Some(target.unit_type);

    let _ = target_id;
}

/// `TaskKillUnit::FindVaildTypes` — unit types able to finish the target
/// (`taskkillunit.cpp:91`): `AiPlayer::GetExtendedWeightTable(target, 0x01)`.
///
/// The C++ request loop that turns the positive-weight entries into
/// `TaskObtainUnits` is ported in [`kill_unit_get_new_units`].
fn kill_unit_find_valid_types(
    world: &crate::game::GameState,
    team: TeamId,
    state: &mut super::KillUnitState,
    target: &crate::unit::UnitInfo,
) {
    state.weight_table =
        world.ai.players[team.0 as usize].get_extended_weight_table(world, target, 0x01);
}

/// `TaskKillUnit::AddUnit` damage bookkeeping: the unit's damage is fixed at
/// the moment it joins and is removed with it.
fn kill_unit_record_damage(
    ctx: &TaskCtx<'_>,
    team: TeamId,
    state: &mut super::KillUnitState,
    unit: UnitId,
) {
    let Some((_, target)) = kill_unit_target(ctx, team, state) else {
        return;
    };
    let Some(attacker) = ctx.unit(unit) else {
        return;
    };

    let damage = get_projected_damage(ctx.world, &attacker, &target);

    state.unit_damages.retain(|entry| entry.unit != unit);
    state.unit_damages.push(UnitDamage { unit, damage });
}

// ---------------------------------------------------------------------------
// TaskKillUnit
// ---------------------------------------------------------------------------

fn kill_unit_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::KillUnit(state) = kind else {
        return;
    };

    let state = state.as_mut();

    if state.spotted_unit.is_none() || kill_unit_target(ctx, header.team, state).is_none() {
        ctx.tm.task_remove_self(ctx.world, header.id);
        return;
    }

    state.managed_unit = None;

    // C++ `BeginTurn`: a target added within reach keeps to the units on site;
    // the others weigh the target anew every turn.
    kill_unit_update_demand(ctx, header, state);

    let target = kill_unit_target(ctx, header.team, state).map(|(_, info)| info);

    if let Some(target) = target {
        kill_unit_find_valid_types(ctx.world, header.team, state, &target);
    } else {
        state.weight_table.clear();
    }

    state.seek_target = true;

    kill_unit_get_new_units(ctx, header, state);
    kill_unit_move_units(ctx, header, state);
}

/// `TaskKillUnit::MoveUnits` (`taskkillunit.cpp:832`) — members that can still
/// move report back to the task.
fn kill_unit_move_units(
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    state: &super::KillUnitState,
) {
    for unit in state.units.clone() {
        let moves = ctx.unit(unit).map(|info| info.speed > 0).unwrap_or(false);

        // `UnitInfo::IsReadyForOrders(this)` (`unitinfo.cpp:3770`).
        let ready = ctx.ready_to_take_orders(unit) && ctx.tm.unit_task(unit) == Some(header.id);

        if moves && ready {
            ctx.remind_move_finished(unit, false);
        }
    }
}

/// `TaskKillUnit::GetNewUnits` (`taskkillunit.cpp:162`) — the closest suitable
/// units join first; when the group still cannot destroy the target, unit types
/// are rolled from the target's weight table and requested through
/// `TaskObtainUnits`.
fn kill_unit_get_new_units(
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    state: &mut super::KillUnitState,
) {
    // Units whose type no longer fits the target's table leave the task.
    for unit_id in state.units.clone() {
        if let Some(info) = ctx.unit(unit_id) {
            if state.weight_table.get_weight(info.unit_type) == 0 {
                ctx.clear_unit_tasks(unit_id, false);
            }
        }
    }

    if i32::from(state.hits) <= kill_unit_total_projected_damage(state) {
        return;
    }

    if ctx.world.turn_counter < state.recruit_turn {
        return;
    }

    if !is_attack_share_left(ctx, header.team, header) {
        return;
    }

    // `FindClosestCombatUnit` over the mobile lists in C++ order: land/sea
    // first, then air; a strictly closer air unit wins.
    loop {
        if i32::from(state.hits) <= kill_unit_total_projected_damage(state) {
            break;
        }

        if !is_attack_share_left(ctx, header.team, header) {
            break;
        }

        let mut best: Option<(i32, UnitId)> = None;

        for list_kind in [UnitListKind::MobileLandSea, UnitListKind::MobileAir] {
            for unit_id in ctx.list(list_kind) {
                if state.units.contains(&unit_id) {
                    continue;
                }

                if !is_unit_suitable(ctx, header.team, state, unit_id) {
                    continue;
                }

                if !crate::ai::task::task_is_recruitable(
                    ctx.tm,
                    ctx.world,
                    unit_id,
                    header.team,
                    ctx.priority_of(header.id),
                ) {
                    continue;
                }

                let Some(info) = ctx.unit(unit_id) else {
                    continue;
                };

                let distance = crate::access::squared_distance(
                    info.grid,
                    state.target_position,
                );

                if best.map(|(best_distance, _)| distance < best_distance).unwrap_or(true) {
                    best = Some((distance, unit_id));
                }
            }
        }

        let Some((_, unit_id)) = best else {
            break;
        };

        // `unit->RemoveTasks()` — the unit leaves its previous task silently.
        ctx.tm.remove_unit_tasks(ctx.world, unit_id);

        state.units.push(unit_id);
        kill_unit_record_damage(ctx, header.team, state, unit_id);
        ctx.attach(header.id, unit_id);
        ctx.remind_move_finished(unit_id, true);
    }

    if i32::from(state.hits) <= kill_unit_total_projected_damage(state) {
        return;
    }

    if !is_attack_share_left(ctx, header.team, header) {
        return;
    }

    if state.unit_requests != 0 {
        return;
    }

    let Some((_, target)) = kill_unit_target(ctx, header.team, state) else {
        return;
    };

    let Some((_, target_position, _)) =
        state.spotted_unit.and_then(|index| spotted_target(ctx, header.team, index))
    else {
        return;
    };

    let mut table = state.weight_table.clone();
    let turns_till_mission_end = crate::ai::task::task_estimate_turns_till_mission_end(ctx.world);
    let current_values = &ctx.world.team_units[header.team.0 as usize];

    for entry in table.entries_mut() {
        let Some(unit_type) = entry.unit_type else {
            continue;
        };

        if current_values
            .current_unit_values(unit_type as u16)
            .get_attribute(crate::unit_values::ATTRIB_TURNS)
            > turns_till_mission_end
            || crate::access::unit_type_flags(unit_type)
                & unit_flags::REGENERATING_UNIT
                != 0
        {
            entry.weight = 0;
        }
    }

    let mut remaining_hits = i32::from(state.hits) - kill_unit_total_projected_damage(state);
    let mut requested = 0;

    // The C++ loop ends when the rolled type cannot reduce the remaining
    // damage; the explicit counter is a safety belt against a table whose
    // entries all deal zero damage.
    while remaining_hits > 0 && requested < 32 {
        let Some(unit_type) = table.roll_unit_type(&mut ctx.world.rng) else {
            break;
        };

        let pseudo = UnitInfo::new(unit_type, header.team, UnitId::NO_ID);
        let damage = get_projected_damage(ctx.world, &pseudo, &target);

        if damage <= 0 {
            break;
        }

        remaining_hits -= damage;
        requested += 1;

        state.unit_requests = state.unit_requests.saturating_add(1);

        let mut obtain_kind = TaskKind::obtain_units(target_position);
        super::create::obtain_units_add_type(&mut obtain_kind, unit_type);
        ctx.append_task(
            header.team,
            Some(header.id),
            ctx.priority_of(header.id),
            obtain_kind,
        );
    }
}

/// `TaskKillUnit::EndTurn` (`taskkillunit.cpp:672`) — `MoveUnits()`
/// (`taskkillunit.cpp:832`): members that can still move report back to the
/// task. Unlike `BeginTurn` this neither recruits nor re-evaluates the target.
fn kill_unit_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::KillUnit(state) = kind else {
        return;
    };

    // `TaskKillUnit::EndTurn` only acts while the target exists.
    if state.spotted_unit.is_none() {
        return;
    }

    kill_unit_move_units(ctx, header, state);
}

fn kill_unit_execute(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let TaskKind::KillUnit(state) = kind else {
        return false;
    };

    if !state.units.contains(&unit) || !ctx.ready_to_take_orders(unit) {
        return false;
    }

    let Some((target_id, _, _)) = state
        .spotted_unit
        .and_then(|index| spotted_target(ctx, header.team, index))
    else {
        return false;
    };

    let Some(attacker) = ctx.unit(unit) else {
        return false;
    };

    // `AiAttack_ProcessAttack` — the attack order (including `ORDER_MOVE_TO_ATTACK`).
    let issued = attack::process_attack(ctx.tm, ctx.world, unit, target_id);

    if issued {
        return true;
    }

    // Not in range: approach the target through a firing site
    // (`TaskAttack::MoveCombatUnit` → `AiAttack_ChooseSiteForAttacker`), not by
    // stepping onto the target cell itself.
    let target_position = state.target_position;
    let range = attacker
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_RANGE);

    let site = attack::choose_site_for_attacker(
        ctx.world,
        &attacker,
        target_position,
        CautionLevel::AvoidNextTurnsFire,
        range,
        true,
    )
    .map(|(site, _)| site)
    .unwrap_or(target_position);

    if site != attacker.grid {
        let priority = ctx.priority_of(header.id);
        let move_kind =
            TaskKind::move_to(unit, site, 0, CautionLevel::AvoidNextTurnsFire);
        ctx.append_task(attacker.team, Some(header.id), priority, move_kind);
    }

    true
}

/// `TaskKillUnit::AddUnit` (`taskkillunit.cpp:607`) — the unit's damage is
/// recorded once, at the moment it joins.
fn kill_unit_add_unit(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) {
    let TaskKind::KillUnit(state) = kind else {
        return;
    };

    if state.units.contains(&unit) {
        return;
    }

    if state.spotted_unit.is_none() {
        ctx.clear_unit_tasks(unit, false);
        return;
    }

    state.units.push(unit);
    kill_unit_record_damage(ctx, header.team, state, unit);
    ctx.attach(header.id, unit);
    ctx.remind_move_finished(unit, true);
}

fn kill_unit_remove_unit(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) {
    let TaskKind::KillUnit(state) = kind else {
        return;
    };

    state.units.retain(|id| *id != unit);

    // The unit takes back exactly the damage it was counted with when it joined.
    state.unit_damages.retain(|damage| damage.unit != unit);

    if state.units.is_empty() {
        ctx.tm.task_remove_self(ctx.world, header.id);
    }
}

fn kill_unit_event_unit_destroyed(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let TaskKind::KillUnit(state) = kind else {
        return false;
    };

    state.units.retain(|id| *id != unit);
    state.unit_damages.retain(|damage| damage.unit != unit);

    let is_target = state
        .spotted_unit
        .and_then(|index| spotted_target(ctx, header.team, index))
        .map(|(target, _, _)| target == unit)
        .unwrap_or(false);

    if is_target || state.units.is_empty() {
        ctx.tm.task_remove_self(ctx.world, header.id);
    }

    true
}

fn kill_unit_event_enemy_spotted(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let TaskKind::KillUnit(state) = kind else {
        return false;
    };

    if state.spotted_unit.is_some() {
        return true;
    }

    let Some(info) = ctx.unit(unit) else {
        return true;
    };

    let player = &ctx.world.ai.players[header.team.0 as usize];

    if let Some(index) = player
        .spotted_units
        .iter()
        .position(|spotted| spotted.unit == Some(unit))
    {
        state.spotted_unit = Some(index as u32);
        state.target_type = Some(info.unit_type);
        state.target_position = info.grid;
        state.seek_target = true;
        kill_unit_update_demand(ctx, header, state);
    }

    true
}

/// `TaskKillUnit::ChildComplete` (`taskkillunit.cpp:661`) — an obtain request
/// finished: the counter drops and the demand is re-evaluated on the next turn.
fn kill_unit_child_complete(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    child: u32,
) {
    let TaskKind::KillUnit(state) = kind else {
        return;
    };

    let is_obtain_request = ctx
        .tm
        .task(child)
        .map(|task| task.get_type() == TASK_TYPE_OBTAIN_UNITS)
        .unwrap_or(false);

    if is_obtain_request {
        state.unit_requests = state.unit_requests.saturating_sub(1);
    }

    let _ = (header, child);
}

// ---------------------------------------------------------------------------
// TaskAttack
// ---------------------------------------------------------------------------

/// `TaskAttack::ChooseFirstTarget` (`taskattack.cpp:1202`) — the first
/// secondary target; when the group is already at its attack zone and the
/// leader is at full speed, the current target is rotated to the back so that
/// the group keeps moving down the target list.
fn attack_choose_first_target(ctx: &TaskCtx<'_>, state: &mut super::AttackState) {
    if state.secondary_targets.is_empty() {
        state.kill_unit_task = None;
        return;
    }

    state.kill_unit_task = state.secondary_targets.first().copied();

    if !state.attack_zone_reached {
        return;
    }

    let full_speed = state
        .leader
        .and_then(|leader| ctx.unit(leader))
        .map(|unit| {
            unit.speed
                == unit
                    .base_values
                    .get_attribute(crate::unit_values::ATTRIB_SPEED)
                    .max(0) as u16
        })
        .unwrap_or(false);

    if !full_speed {
        return;
    }

    let previous = state.kill_unit_task;
    let first = state.secondary_targets.remove(0);
    state.secondary_targets.push(first);
    state.kill_unit_task = state.secondary_targets.first().copied();

    if state.kill_unit_task != previous {
        state.attack_zone_reached = false;
    }
}

/// `TaskAttack` constructor: the first spotted enemy of the target team.
fn attack_choose_initial_target(
    ctx: &TaskCtx<'_>,
    team: TeamId,
    target_team: TeamId,
) -> Option<u32> {
    let player = &ctx.world.ai.players[team.0 as usize];

    for (index, spotted) in player.spotted_units.iter().enumerate() {
        let Some(unit_id) = spotted.unit else {
            continue;
        };
        let Some(info) = ctx.unit(unit_id) else {
            continue;
        };

        if info.team == target_team && info.hits > 0 {
            return Some(index as u32);
        }
    }

    None
}

/// Creates the `TaskKillUnit` for a spotted target (`TaskAttack` constructor
/// and `AssessEnemyUnits`). The constructor values of `required_damage`/`hits`
/// are set immediately so that the first `EvaluateAttackReadiness` already sees
/// a correct census.
fn attack_create_kill_task(
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    state: &mut super::AttackState,
    spotted_index: u32,
) -> Option<u32> {
    let (target_id, position, unit_type) = spotted_target(ctx, header.team, spotted_index)?;
    let target = ctx.unit(target_id)?;

    let mut kill = super::KillUnitState::default();
    kill.spotted_unit = Some(spotted_index);
    kill.target_position = position;
    kill.target_type = Some(unit_type);
    kill.seek_target = true;
    kill.recruit_turn = 0;

    let target_ammo = target
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_AMMO);

    if target_ammo > 0 && target.team != TeamId::ALIEN {
        kill.required_damage = target.hits;
        kill.hits = target.hits.saturating_mul(2);
    } else {
        kill.required_damage = 1;
        kill.hits = target.hits;
    }

    let priority = ctx.priority_of(header.id);
    let task_id = ctx.append_task(
        header.team,
        Some(header.id),
        priority,
        TaskKind::KillUnit(Box::new(kill)),
    );

    if let Some(spot) = ctx.world.ai.players[header.team.0 as usize]
        .spotted_units
        .get_mut(spotted_index as usize)
    {
        spot.task = Some(header.id);
    }

    state.kill_unit_task = Some(task_id);
    state.primary_targets.push(task_id);
    state.secondary_targets.push(task_id);

    Some(task_id)
}

/// The current target of the attack (`kill_unit_task`).
fn attack_current_target(
    ctx: &TaskCtx<'_>,
    team: TeamId,
    state: &super::AttackState,
) -> Option<(UnitId, UnitInfo)> {
    let kill_id = state.kill_unit_task?;
    let task = ctx.tm.task(kill_id)?;
    let TaskKind::KillUnit(kill) = &task.kind else {
        return None;
    };

    kill_unit_target(ctx, team, kill)
}

/// `TaskAttack::IsWithinAttackRange` (`taskattack.cpp:1085`).
fn attack_is_within_attack_range(unit: &UnitInfo, position: Point) -> bool {
    if unit.ammo == 0 {
        return false;
    }

    let distance = crate::combat::get_attack_range(unit) - 3;
    let distance = distance * distance;

    crate::access::squared_distance(unit.grid, position) <= distance
}

/// `TaskAttack::IsWithinAttackRange` for a kill task's target.
fn attack_target_within_range(
    ctx: &TaskCtx<'_>,
    team: TeamId,
    kill: &super::KillUnitState,
    position: Point,
) -> bool {
    kill_unit_target(ctx, team, kill)
        .map(|(_, target)| attack_is_within_attack_range(&target, position))
        .unwrap_or(false)
}

/// `TaskAttack::GetSafeDistances` (`taskattack.cpp:1102`) — the scan distances
/// at which the group's spotters see the defenders covering the current target.
fn attack_safe_distances(
    tm: &super::super::task_manager::TaskManager,
    world: &crate::game::GameState,
    team: TeamId,
    state: &super::AttackState,
) -> (i32, i32) {
    let mut safe_distance_air = 0;
    let mut safe_distance_ground = 0;

    let Some(kill_id) = state.kill_unit_task else {
        return (0, 0);
    };

    let Some(kill) = tm.task(kill_id).and_then(|task| match &task.kind {
        TaskKind::KillUnit(kill) => Some(kill.as_ref()),
        _ => None,
    }) else {
        return (0, 0);
    };

    let unit_position = kill.target_position;

    for task_id in &state.secondary_targets {
        let Some(task) = tm.task(*task_id) else {
            continue;
        };
        let TaskKind::KillUnit(other) = &task.kind else {
            continue;
        };

        let Some((target_id, _, _)) = other
            .spotted_unit
            .and_then(|index| spotted_target_for_ai(world, team, index))
        else {
            continue;
        };

        let Some(target) = world.unit(target_id) else {
            continue;
        };

        if target
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_ATTACK)
            <= 0
        {
            continue;
        }

        if *task_id != kill_id && !attack_is_within_attack_range(target, unit_position) {
            continue;
        }

        let range = target
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_RANGE);

        if crate::access::is_valid_attack_target_type(target.unit_type, UnitType::Awac) {
            safe_distance_air = safe_distance_air.max(range + 1);
        } else {
            safe_distance_ground = safe_distance_ground.max(range + 1);
        }
    }

    let team_values = &world.team_units[team.0 as usize];
    let awac_scan = team_values
        .current_unit_values(UnitType::Awac as u16)
        .get_attribute(crate::unit_values::ATTRIB_SCAN);

    safe_distance_air -= awac_scan;

    let ground_scan = if state.access_flags & unit_flags::MOBILE_LAND_UNIT != 0 {
        team_values
            .current_unit_values(UnitType::Scanner as u16)
            .get_attribute(crate::unit_values::ATTRIB_SCAN)
    } else {
        team_values
            .current_unit_values(UnitType::Escort as u16)
            .get_attribute(crate::unit_values::ATTRIB_SCAN)
    };

    safe_distance_ground -= ground_scan;

    if safe_distance_air > 0 && safe_distance_ground > 0 {
        let safe_distance = safe_distance_air.min(safe_distance_ground);
        safe_distance_air -= safe_distance;
        safe_distance_ground -= safe_distance;
    }

    (safe_distance_air, safe_distance_ground)
}

/// `TaskAttack::IsReconUnitUsable` (`taskattack.cpp:1060`).
fn attack_is_recon_unit_usable(
    _world: &crate::game::GameState,
    _team: TeamId,
    state: &super::AttackState,
    unit: &UnitInfo,
) -> bool {
    if !matches!(
        unit.unit_type,
        UnitType::Awac | UnitType::Scout | UnitType::Scanner | UnitType::Escort
    ) || crate::ai::task::task_is_repair_due(unit, true)
    {
        return false;
    }

    let safe_distance_air = state.safe_distance_air;
    let safe_distance_ground = state.safe_distance_ground;
    let scan = unit
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_SCAN);

    if unit.unit_type == UnitType::Awac {
        scan >= safe_distance_air
    } else {
        scan >= safe_distance_ground
    }
}

/// `TaskAttack::IsReconUnitAvailable` (`taskattack.cpp:1915`).
fn attack_is_recon_unit_available(
    tm: &super::super::task_manager::TaskManager,
    world: &crate::game::GameState,
    team: TeamId,
    state: &super::AttackState,
) -> bool {
    if state.recon_unit.is_some() {
        return true;
    }

    let (safe_distance_air, safe_distance_ground) = attack_safe_distances(tm, world, team, state);

    for task_id in &state.secondary_targets {
        let Some(task) = tm.task(*task_id) else {
            continue;
        };
        let TaskKind::KillUnit(kill) = &task.kind else {
            continue;
        };

        for unit_id in &kill.units {
            let Some(unit) = world.unit(*unit_id) else {
                continue;
            };
            let scan = unit
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_SCAN);

            if unit.flags & unit_flags::MOBILE_AIR_UNIT != 0 {
                if scan >= safe_distance_air {
                    return true;
                }
            } else if scan >= safe_distance_ground {
                return true;
            }
        }
    }

    false
}

/// `TaskAttack::IsAnyTargetInRange` (`taskattack.cpp:1897`).
fn attack_is_any_target_in_range(
    tm: &super::super::task_manager::TaskManager,
    world: &crate::game::GameState,
    team: TeamId,
    state: &super::AttackState,
) -> bool {
    for task_id in &state.secondary_targets {
        let Some(task) = tm.task(*task_id) else {
            continue;
        };
        let TaskKind::KillUnit(kill) = &task.kind else {
            continue;
        };

        for unit_id in &kill.units {
            let Some(unit) = world.unit(*unit_id) else {
                continue;
            };

            let unit_range = unit
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_RANGE);
            let unit_range = unit_range * unit_range;

            for other_id in &state.secondary_targets {
                let Some(other) = tm.task(*other_id) else {
                    continue;
                };
                let TaskKind::KillUnit(other_kill) = &other.kind else {
                    continue;
                };

                if let Some((_, target)) = kill_unit_target_from_world(world, team, other_kill)
                {
                    if crate::access::squared_distance(unit.grid, target.grid) <= unit_range {
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// `kill_unit_target` for the read-only `aiattack.cpp` layer.
fn kill_unit_target_from_world(
    world: &crate::game::GameState,
    team: TeamId,
    state: &super::KillUnitState,
) -> Option<(UnitId, UnitInfo)> {
    let (unit_id, _, _) = spotted_target_for_ai(world, team, state.spotted_unit?)?;
    Some((unit_id, world.unit(unit_id)?.clone()))
}

/// `TaskAttack::IsTargetGroupInSight` (`taskattack.cpp:1242`).
fn attack_is_target_group_in_sight(
    tm: &super::super::task_manager::TaskManager,
    world: &crate::game::GameState,
    team: TeamId,
    state: &super::AttackState,
) -> bool {
    let Some(first_task_id) = state.secondary_targets.first().copied() else {
        return false;
    };
    let Some(first_task) = tm.task(first_task_id) else {
        return false;
    };
    let TaskKind::KillUnit(first_kill) = &first_task.kind else {
        return false;
    };
    let Some((_, first_target)) = kill_unit_target_from_world(world, team, first_kill) else {
        return false;
    };
    let target_team = first_target.team;

    for task_id in &state.secondary_targets {
        let Some(task) = tm.task(*task_id) else {
            continue;
        };
        let TaskKind::KillUnit(kill) = &task.kind else {
            continue;
        };

        for unit_id in &kill.units {
            let Some(unit) = world.unit(*unit_id) else {
                continue;
            };
            if unit.is_visible_to_team(target_team) {
                return true;
            }
        }
    }

    false
}

/// `TaskAttack::IsAttackUnderControl` (`taskattack.cpp:1595`) — whether the
/// damage the group can deal at the target covers the damage needed against it
/// and its covering defenders.
fn attack_is_attack_under_control(
    tm: &super::super::task_manager::TaskManager,
    world: &crate::game::GameState,
    team: TeamId,
    state: &super::AttackState,
) -> bool {
    let Some(kill_id) = state.kill_unit_task else {
        return false;
    };
    let Some(kill_task) = tm.task(kill_id) else {
        return false;
    };
    let TaskKind::KillUnit(kill) = &kill_task.kind else {
        return false;
    };
    let Some((_, target)) = kill_unit_target_from_world(world, team, kill) else {
        return false;
    };

    let target_position = kill.target_position;
    let mut required_damage = 0;
    let mut projected_damage = 0;

    for task_id in &state.secondary_targets {
        let Some(task) = tm.task(*task_id) else {
            continue;
        };
        let TaskKind::KillUnit(other) = &task.kind else {
            continue;
        };

        if *task_id == kill_id {
            required_damage += i32::from(other.required_damage);
        } else if let Some((_, other_target)) =
            kill_unit_target_from_world(world, team, other)
        {
            if attack_is_within_attack_range(&other_target, target_position) {
                required_damage += i32::from(other.required_damage);
            }
        }

        projected_damage += kill_unit_total_projected_damage(other);
    }

    if projected_damage < required_damage {
        return false;
    }

    if target.is_visible_to_team(team) {
        return true;
    }

    // Not visible: a spotter at its attack site suffices, otherwise the group's
    // best scan must cover the most valuable member's target value.
    if let Some(recon_id) = state.recon_unit {
        if let Some(recon) = world.unit(recon_id) {
            if recon.orders == UnitOrder::Await
                || recon.orders == UnitOrder::Sentry
                || recon.target_grid == recon.grid
            {
                return true;
            }
        }
    }

    let mut max_unit_worth = 0;
    let mut max_unit_scan = 0;

    for task_id in &state.secondary_targets {
        let Some(task) = world.ai.task_manager.task(*task_id) else {
            continue;
        };
        let TaskKind::KillUnit(other) = &task.kind else {
            continue;
        };

        for unit_id in &other.units {
            let Some(unit) = world.unit(*unit_id) else {
                continue;
            };
            max_unit_worth = max_unit_worth.max(attack::get_target_value(unit));
            max_unit_scan = max_unit_scan.max(
                unit.base_values
                    .get_attribute(crate::unit_values::ATTRIB_SCAN),
            );
        }
    }

    max_unit_worth <= max_unit_scan
}

/// `TaskAttack::IsViableLeader` (`taskattack.cpp:1729`).
fn attack_is_viable_leader(
    ctx: &TaskCtx<'_>,
    team: TeamId,
    state: &super::AttackState,
    unit: &UnitInfo,
) -> bool {
    if unit.hits == 0 {
        return false;
    }

    let Some((_, target)) = attack_current_target(ctx, team, state) else {
        return true;
    };

    if target.is_visible_to_team(team) {
        if matches!(
            unit.unit_type,
            UnitType::Infiltrator | UnitType::Submarine
        ) {
            let scan = unit
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_SCAN);
            crate::access::squared_distance(unit.grid, target.grid) <= scan * scan
        } else {
            true
        }
    } else if state.recon_unit.is_some() && state.recon_unit != Some(unit.id) {
        false
    } else {
        attack::get_target_value(unit)
            <= unit
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_SCAN)
    }
}

/// `TaskAttack::DetermineLeader` (`taskattack.cpp:740`) — the viable unit
/// closest to the current target; the recon unit is the initial candidate.
fn attack_determine_leader(
    ctx: &TaskCtx<'_>,
    team: TeamId,
    state: &mut super::AttackState,
) -> Option<UnitId> {
    if let Some(leader) = state.leader {
        if let Some(info) = ctx.unit(leader) {
            if attack_is_viable_leader(ctx, team, state, &info) {
                return Some(leader);
            }
        }
    }

    state.attack_zone_reached = false;

    let position = attack_current_target(ctx, team, state)
        .map(|(_, target)| target.grid)
        .unwrap_or(Point::ZERO);

    let mut leader: Option<UnitId> = None;
    let mut minimum_distance = i32::MAX;

    if let Some(recon) = state.recon_unit {
        if let Some(info) = ctx.unit(recon) {
            leader = Some(recon);
            minimum_distance = crate::access::squared_distance(info.grid, position);
        }
    }

    for task_id in &state.secondary_targets {
        let Some(TaskKind::KillUnit(kill)) =
            ctx.tm.task(*task_id).map(|task| &task.kind)
        else {
            continue;
        };

        for unit_id in &kill.units {
            let Some(info) = ctx.unit(*unit_id) else {
                continue;
            };
            if !attack_is_viable_leader(ctx, team, state, &info) {
                continue;
            }

            let distance = crate::access::squared_distance(info.grid, position);
            if state.recon_unit.is_none() || distance < minimum_distance {
                leader = Some(*unit_id);
                minimum_distance = distance;
            }
        }
    }

    if leader.is_none() {
        for task_id in &state.secondary_targets {
            let Some(TaskKind::KillUnit(kill)) =
                ctx.tm.task(*task_id).map(|task| &task.kind)
            else {
                continue;
            };

            for unit_id in &kill.units {
                let Some(info) = ctx.unit(*unit_id) else {
                    continue;
                };
                let distance = crate::access::squared_distance(info.grid, position);

                if leader.is_none() || distance < minimum_distance {
                    leader = Some(*unit_id);
                    minimum_distance = distance;
                }
            }
        }
    }

    state.leader = leader;
    state.leader_task = None;

    leader
}

/// `TaskAttack::MoveUnits` (`taskattack.cpp:1874`) — the leader and the kill
/// task members report that they can move again.
fn attack_move_units(ctx: &mut TaskCtx<'_>, team: TeamId, state: &mut super::AttackState) {
    if let Some(leader) = attack_determine_leader(ctx, team, state) {
        ctx.remind_move_finished(leader, true);
    }

    for task_id in state.secondary_targets.clone() {
        if let Some(TaskKind::KillUnit(kill)) =
            ctx.tm.task(task_id).map(|task| &task.kind)
        {
            for unit_id in kill.units.clone() {
                let moves = ctx.unit(unit_id).map(|info| info.speed > 0).unwrap_or(false);
                let ready = ctx.ready_to_take_orders(unit_id)
                    && ctx.tm.unit_task(unit_id) == Some(task_id);

                if moves && ready {
                    ctx.remind_move_finished(unit_id, false);
                }
            }
        }
    }
}

/// `TaskAttack::EvaluateAttackReadiness` (`taskattack.cpp:1951`).
fn attack_evaluate_readiness(
    state: &mut super::AttackState,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
) {
    if state.op_state < attack_state::WAIT {
        state.op_state = attack_state::WAIT;
    }

    let Some(kill_id) = state.kill_unit_task else {
        if state.op_state > attack_state::WAIT {
            state.op_state = attack_state::WAIT;
        }
        return;
    };
    let Some((_, target)) = attack_current_target(ctx, header.team, state) else {
        if state.op_state > attack_state::WAIT {
            state.op_state = attack_state::WAIT;
        }
        return;
    };
    let target_visible = target.is_visible_to_team(header.team);

    if !target_visible
        && !attack_is_recon_unit_available(ctx.tm, ctx.world, header.team, state)
    {
        state.op_state = attack_state::WAIT;
        return;
    }

    let target_position = target.grid;
    let mut required_damage = 0;
    let mut projected_damage = 0;

    for task_id in state.secondary_targets.clone() {
        let Some(TaskKind::KillUnit(kill)) =
            ctx.tm.task(task_id).map(|task| &task.kind)
        else {
            continue;
        };

        if task_id == kill_id || attack_target_within_range(ctx, header.team, kill, target_position)
        {
            required_damage += i32::from(kill.required_damage);
        }

        projected_damage += kill_unit_total_projected_damage(kill);
    }

    if projected_damage >= required_damage {
        if state.op_state == attack_state::WAIT {
            state.op_state = attack_state::GATHER_FORCES;
            state.attack_zone_reached = false;
        }
    } else if state.op_state > attack_state::WAIT
        && (state.op_state < attack_state::ATTACK
            || !attack_is_any_target_in_range(ctx.tm, ctx.world, header.team, state))
    {
        state.op_state = attack_state::WAIT;
    }

    if state.op_state == attack_state::NORMAL_SEARCH
        && !attack_is_target_group_in_sight(ctx.tm, ctx.world, header.team, state)
    {
        state.op_state = attack_state::ADVANCE;
    }

    if state.op_state >= attack_state::GATHER_FORCES
        && state.op_state < attack_state::NORMAL_SEARCH
    {
        // C++ learns `attack_zone_reached` from `TaskAttack::MoveUnit`; the
        // reduced attack has no group march, so "the leader is close enough to
        // fire at the target" plays the role of reaching the attack site.
        if let Some(leader) = state.leader.and_then(|leader| ctx.unit(leader)) {
            let range = leader
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_RANGE);

            if crate::access::squared_distance(leader.grid, target_position) <= range * range {
                state.attack_zone_reached = true;
            }
        }

        let leader = attack_determine_leader(ctx, header.team, state);
        let leader_ready = leader
            .and_then(|leader| {
                ctx.unit(leader).map(|info| {
                    info.speed > 0
                        && crate::ai::task::task_is_ready_to_take_orders(ctx.world, leader)
                })
            })
            .unwrap_or(false);

        if state.attack_zone_reached
            && leader_ready
            && attack_is_attack_under_control(ctx.tm, ctx.world, header.team, state)
        {
            state.op_state += 1;
            state.attack_zone_reached = false;

            attack_move_units(ctx, header.team, state);
        }

        let mut in_assault_range = false;

        for task_id in &state.secondary_targets {
            let Some(TaskKind::KillUnit(kill)) =
                ctx.tm.task(*task_id).map(|task| &task.kind)
            else {
                continue;
            };

            for unit_id in &kill.units {
                let Some(unit) = ctx.unit(*unit_id) else {
                    continue;
                };
                let unit_range = crate::combat::get_attack_range(&unit);

                if crate::access::squared_distance(unit.grid, target_position)
                    <= unit_range * unit_range
                {
                    in_assault_range = true;
                    break;
                }
            }

            if in_assault_range {
                break;
            }
        }

        if in_assault_range {
            state.op_state = attack_state::NORMAL_SEARCH;
            state.attack_zone_reached = false;

            attack_move_units(ctx, header.team, state);
        }
    }

    if state.op_state == attack_state::NORMAL_SEARCH
        || state.op_state == attack_state::BOLD_SEARCH
    {
        let visible = attack_current_target(ctx, header.team, state)
            .map(|(_, target)| target.is_visible_to_team(header.team))
            .unwrap_or(false);

        if visible {
            state.op_state = attack_state::ATTACK;
            state.attack_zone_reached = false;

            attack_move_units(ctx, header.team, state);
        }
    }

    if state.op_state == attack_state::ATTACK
        && !attack_current_target(ctx, header.team, state)
            .map(|(_, target)| target.is_visible_to_team(header.team))
            .unwrap_or(false)
    {
        state.op_state = attack_state::NORMAL_SEARCH;
        state.attack_zone_reached = false;

        attack_move_units(ctx, header.team, state);
    }
}

/// `TaskAttack::AssessEnemyUnits` (`taskattack.cpp:2105`) — keep only relevant
/// primary targets and add the defenders that cover them.
///
/// The C++ merge of two attack tasks and the `TaskSupportAttack` spawn are not
/// ported; a spotted unit that is already handled by another task is left to it.
fn attack_assess_enemy_units(
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    state: &mut super::AttackState,
) {
    // Reset the target links first; relevant ones are re-established below.
    for task_id in &state.secondary_targets {
        let Some(TaskKind::KillUnit(kill)) =
            ctx.tm.task(*task_id).map(|task| &task.kind)
        else {
            continue;
        };

        if let Some(index) = kill.spotted_unit {
            if let Some(spot) = ctx.world.ai.players[header.team.0 as usize]
                .spotted_units
                .get_mut(index as usize)
            {
                spot.task = None;
            }
        }
    }

    let mut base_priority = ctx.priority_of(header.id);
    let mut remove = Vec::new();

    for task_id in state.primary_targets.clone() {
        let Some(TaskKind::KillUnit(kill)) =
            ctx.tm.task(task_id).map(|task| &task.kind)
        else {
            remove.push(task_id);
            continue;
        };

        let Some((target_id, target)) = kill_unit_target(ctx, header.team, kill) else {
            remove.push(task_id);
            continue;
        };

        let is_relevant = if target.flags & unit_flags::BUILDING != 0
            || target.unit_type == UnitType::Constructor
        {
            let is_enemy = ctx.is_enemy_team(target.team);

            if is_enemy
                && base_priority > TASK_PRIORITY_ATTACK_PRIORITY_TARGET
                && attack::is_priority_target(ctx.world, &target, header.team)
            {
                base_priority = TASK_PRIORITY_ATTACK_PRIORITY_TARGET;
            }

            is_enemy
        } else if target.team == TeamId::ALIEN {
            true
        } else if attack::is_within_reach(ctx.world, &target, header.team) {
            base_priority = TASK_PRIORITY_ATTACK_MOBILE;
            true
        } else {
            false
        };

        if is_relevant {
            if let Some(spot) = ctx.world.ai.players[header.team.0 as usize]
                .spotted_units
                .get_mut(kill.spotted_unit.unwrap_or(0) as usize)
            {
                spot.task = Some(header.id);
            }
        } else {
            remove.push(task_id);
        }

        let _ = target_id;
    }

    for task_id in remove {
        attack_remove_kill_task(ctx, header, state, task_id);
    }

    let mut remove = Vec::new();

    for task_id in state.secondary_targets.clone() {
        let Some(TaskKind::KillUnit(kill)) =
            ctx.tm.task(task_id).map(|task| &task.kind)
        else {
            remove.push(task_id);
            continue;
        };

        let Some((target_id, _)) = kill_unit_target(ctx, header.team, kill) else {
            remove.push(task_id);
            continue;
        };

        let linked = kill
            .spotted_unit
            .and_then(|index| {
                ctx.world.ai.players[header.team.0 as usize]
                    .spotted_units
                    .get(index as usize)
            })
            .map(|spot| spot.task == Some(header.id))
            .unwrap_or(false);

        if linked || attack_is_defender_dangerous(ctx, header.team, state, target_id) {
            if let Some(spot) = ctx.world.ai.players[header.team.0 as usize]
                .spotted_units
                .get_mut(kill.spotted_unit.unwrap_or(0) as usize)
            {
                spot.task = Some(header.id);
            }
        } else {
            remove.push(task_id);
        }
    }

    for task_id in remove {
        attack_remove_kill_task(ctx, header, state, task_id);
    }

    // Enemy units covering the primary targets join the attack.
    let candidates: Vec<(u32, UnitId)> = {
        let player = &ctx.world.ai.players[header.team.0 as usize];
        player
            .spotted_units
            .iter()
            .enumerate()
            .filter_map(|(index, spotted)| {
                let unit_id = spotted.unit?;
                Some((index as u32, unit_id))
            })
            .collect()
    };

    for (index, target_id) in candidates {
        let Some(target) = ctx.unit(target_id) else {
            continue;
        };

        if target.hits == 0
            || target.team == header.team
            || !ctx.is_enemy_team(target.team)
            || !attack_is_defender_dangerous(ctx, header.team, state, target_id)
        {
            continue;
        }

        let linked = ctx.world.ai.players[header.team.0 as usize]
            .spotted_units
            .get(index as usize)
            .map(|spot| spot.task.is_some())
            .unwrap_or(true);

        if linked {
            continue;
        }

        let mut kill = super::KillUnitState::default();
        kill.spotted_unit = Some(index);
        kill.target_position = target.grid;
        kill.target_type = Some(target.unit_type);
        kill.seek_target = true;
        kill.recruit_turn = 0;

        let target_ammo = target
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_AMMO);

        if target_ammo > 0 && target.team != TeamId::ALIEN {
            kill.required_damage = target.hits;
            kill.hits = target.hits.saturating_mul(2);
        } else {
            kill.required_damage = 1;
            kill.hits = target.hits;
        }

        let task_id = ctx.append_task(
            header.team,
            Some(header.id),
            base_priority,
            TaskKind::KillUnit(Box::new(kill)),
        );

        if let Some(spot) = ctx.world.ai.players[header.team.0 as usize]
            .spotted_units
            .get_mut(index as usize)
        {
            spot.task = Some(header.id);
        }

        state.secondary_targets.push(task_id);
    }

    for task_id in state.secondary_targets.clone() {
        ctx.set_priority(task_id, base_priority);
    }
}

/// `TaskAttack::IsDefenderDangerous` (`taskattack.cpp:2072`) — the defender
/// can fire at one of the primary targets.
fn attack_is_defender_dangerous(
    ctx: &TaskCtx<'_>,
    team: TeamId,
    state: &super::AttackState,
    target_id: UnitId,
) -> bool {
    let Some(target) = ctx.unit(target_id) else {
        return false;
    };

    if target.ammo == 0 {
        return false;
    }

    let range = crate::combat::get_attack_range(&target) - 3;
    let range = range * range;

    for task_id in &state.primary_targets {
        let Some(TaskKind::KillUnit(kill)) =
            ctx.tm.task(*task_id).map(|task| &task.kind)
        else {
            continue;
        };

        if let Some((_, primary_target)) = kill_unit_target(ctx, team, kill) {
            if crate::access::squared_distance(primary_target.grid, target.grid) <= range {
                return true;
            }
        }
    }

    false
}

/// `TaskAttack::RemoveTask` (`taskattack.cpp:2099`) — drop a kill task and
/// release its units.
fn attack_remove_kill_task(
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    state: &mut super::AttackState,
    task_id: u32,
) {
    let units = if let Some(TaskKind::KillUnit(kill)) =
        ctx.tm.task(task_id).map(|task| &task.kind)
    {
        if let Some(index) = kill.spotted_unit {
            if let Some(spot) = ctx.world.ai.players[header.team.0 as usize]
                .spotted_units
                .get_mut(index as usize)
            {
                if spot.task == Some(header.id) {
                    spot.task = None;
                }
            }
        }

        kill.units.clone()
    } else {
        Vec::new()
    };

    state.primary_targets.retain(|id| *id != task_id);
    state.secondary_targets.retain(|id| *id != task_id);

    if state.kill_unit_task == Some(task_id) {
        state.kill_unit_task = None;
    }

    // Remove the task before releasing its units: otherwise the unit cleanup
    // re-enters `TaskKillUnit::RemoveUnit` and, with an empty list, notifies this
    // attack about a child that is being removed anyway.
    ctx.tm.remove_task(ctx.world, task_id);

    for unit_id in units {
        ctx.tm
            .clear_unit_tasks_and_remind_available(ctx.world, unit_id, false);
    }
}

fn attack_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::Attack(state) = kind else {
        return;
    };

    if state.access_flags == 0 {
        // `TaskAttack::Init` (`taskattack.cpp:271`).
        state.access_flags = unit_flags::MOBILE_AIR_UNIT
            | unit_flags::MOBILE_SEA_UNIT
            | unit_flags::MOBILE_LAND_UNIT;
    }

    if state.kill_unit_task.is_none() && state.secondary_targets.is_empty() {
        let target_team = state.target_team.unwrap_or(header.team);

        let initial_target = state
            .initial_target
            .filter(|index| {
                spotted_target(ctx, header.team, *index)
                    .and_then(|(unit_id, _, _)| ctx.unit(unit_id))
                    .map(|unit| unit.team == target_team && unit.hits > 0)
                    .unwrap_or(false)
            })
            .or_else(|| attack_choose_initial_target(ctx, header.team, target_team));

        state.initial_target = None;

        if let Some(index) = initial_target {
            attack_create_kill_task(ctx, header, state, index);
        }
    } else if state.kill_unit_task.is_none() {
        attack_choose_first_target(ctx, state);
    } else if let Some(task_id) = state.kill_unit_task {
        if !ctx.tm.has_task(task_id) {
            state.kill_unit_task = None;
            attack_choose_first_target(ctx, state);
        }
    }

    attack_assess_enemy_units(ctx, header, state);

    if state.primary_targets.is_empty() {
        state.secondary_targets.clear();
        ctx.remove_self(header.id);
        return;
    }

    attack_choose_first_target(ctx, state);
    let (safe_air, safe_ground) = attack_safe_distances(ctx.tm, ctx.world, header.team, state);
    state.safe_distance_air = safe_air;
    state.safe_distance_ground = safe_ground;
    attack_update_recon_unit(ctx, header, state);

    state.leader = attack_determine_leader(ctx, header.team, state);
    state.attack_zone_reached = false;
    state.defenders_fire = 0;

    if !state.secondary_targets.is_empty() {
        attack_evaluate_readiness(state, ctx, header);
    }
}

/// `TaskAttack::EndTurn` (`taskattack.cpp:451`) — readiness first, then the
/// turn-end reminders that let idle group members report back to the attack.
fn attack_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::Attack(state) = kind else {
        return;
    };

    attack_evaluate_readiness(state, ctx, header);

    if let Some(recon_unit) = state.recon_unit {
        if ctx
            .unit(recon_unit)
            .map(|info| info.speed > 0)
            .unwrap_or(false)
        {
            ctx.remind_move_finished(recon_unit, true);
        }
    }

    // Support-attack and secondary-target units (`taskattack.cpp:458-473`).
    let mut units: Vec<UnitId> = Vec::new();

    if let Some(task_id) = state.support_attack_task {
        if let Some(task) = ctx.tm.task(task_id) {
            if let TaskKind::SupportAttack {
                units: support_units,
                ..
            } = &task.kind
            {
                units.extend(support_units.iter().copied());
            }
        }
    }

    for task_id in state.secondary_targets.clone() {
        if let Some(task) = ctx.tm.task(task_id) {
            if let TaskKind::KillUnit(kill) = &task.kind {
                units.extend(kill.units.iter().copied());
            }
        }
    }

    for unit in units {
        let moves = ctx.unit(unit).map(|info| info.speed > 0).unwrap_or(false);

        // `UnitInfo::IsReadyForOrders(this)` (`unitinfo.cpp:3770`).
        let ready = ctx.ready_to_take_orders(unit) && ctx.tm.unit_task(unit) == Some(header.id);

        if moves && ready {
            ctx.remind_move_finished(unit, true);
        }
    }
}

/// `TaskAttack::AddUnit` (`taskattack.cpp:243`) — the attack itself takes only
/// a spotter; combat units are handled by the kill tasks or the reserve.
fn attack_add_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    let TaskKind::Attack(state) = kind else {
        return;
    };

    let Some(info) = ctx.unit(unit) else {
        return;
    };

    // Refresh the cached safe distances for `IsReconUnitUsable`.
    let (safe_air, safe_ground) = attack_safe_distances(ctx.tm, ctx.world, header.team, state);
    state.safe_distance_air = safe_air;
    state.safe_distance_ground = safe_ground;

    if state.recon_unit.is_none()
        && attack_is_recon_unit_usable(ctx.world, header.team, state, &info)
    {
        state.recon_unit = Some(unit);
        ctx.attach(header.id, unit);
        ctx.remind_move_finished(unit, true);
    } else {
        ctx.tm
            .clear_unit_tasks_and_remind_available(ctx.world, unit, false);
    }
}

/// `TaskAttack::UpdateReconUnit` (`taskattack.cpp:1157`) — keep or replace the
/// spotter so that it outranges the defenders around the target.
fn attack_update_recon_unit(
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    state: &mut super::AttackState,
) {
    if state.kill_unit_task.is_none() {
        return;
    }

    let (safe_air, safe_ground) = attack_safe_distances(ctx.tm, ctx.world, header.team, state);

    if let Some(recon_id) = state.recon_unit {
        if let Some(recon) = ctx.unit(recon_id) {
            let scan = recon
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_SCAN);

            if recon.unit_type == UnitType::Awac {
                if scan >= safe_air {
                    return;
                }
            } else if scan >= safe_ground {
                return;
            }
        }
    }

    if state.access_flags & unit_flags::MOBILE_SEA_UNIT != 0 {
        if !attack_find_recon_unit(ctx, header, state, UnitType::Escort, safe_ground)
            && !attack_find_recon_unit(ctx, header, state, UnitType::Awac, safe_air)
        {
            attack_request_recon_unit(ctx, header, state, UnitType::Escort, safe_ground);
            attack_request_recon_unit(ctx, header, state, UnitType::Awac, safe_air);
        }
    } else if state.access_flags & unit_flags::MOBILE_LAND_UNIT != 0 {
        if !attack_find_recon_unit(ctx, header, state, UnitType::Scout, safe_ground)
            && !attack_find_recon_unit(ctx, header, state, UnitType::Scanner, safe_ground)
            && !attack_find_recon_unit(ctx, header, state, UnitType::Awac, safe_air)
            && !attack_request_recon_unit(ctx, header, state, UnitType::Scout, safe_ground)
        {
            attack_request_recon_unit(ctx, header, state, UnitType::Scanner, safe_ground);
            attack_request_recon_unit(ctx, header, state, UnitType::Awac, safe_air);
        }
    } else if !attack_find_recon_unit(ctx, header, state, UnitType::Awac, safe_air) {
        attack_request_recon_unit(ctx, header, state, UnitType::Awac, 0);
    }
}

/// `TaskAttack::FindReconUnit` (`taskattack.cpp:1009`).
fn attack_find_recon_unit(
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    state: &mut super::AttackState,
    unit_type: UnitType,
    safe_distance: i32,
) -> bool {
    let team = header.team;
    let current_values = &ctx.world.team_units[team.0 as usize];

    if current_values
        .current_unit_values(unit_type as u16)
        .get_attribute(crate::unit_values::ATTRIB_SCAN)
        < safe_distance
    {
        return false;
    }

    let list_kind = if crate::access::unit_type_flags(unit_type)
        & unit_flags::MOBILE_AIR_UNIT
        != 0
    {
        UnitListKind::MobileAir
    } else {
        UnitListKind::MobileLandSea
    };

    let mut new_recon: Option<UnitId> = None;

    for unit_id in ctx.list(list_kind) {
        let Some(info) = ctx.unit(unit_id) else {
            continue;
        };

        if info.team != team
            || info.unit_type != unit_type
            || info
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_SCAN)
                < safe_distance
            || crate::ai::task::task_is_repair_due(&info, true)
        {
            continue;
        }

        if !crate::ai::task::task_is_recruitable(
            ctx.tm,
            ctx.world,
            unit_id,
            team,
            ctx.priority_of(header.id),
        ) {
            continue;
        }

        let has_task = ctx.tm.unit_task(unit_id).is_some();
        let prefer = match new_recon {
            None => true,
            Some(candidate) => ctx.tm.unit_task(candidate).is_some() && !has_task,
        };

        if prefer {
            new_recon = Some(unit_id);
        }
    }

    let Some(new_recon) = new_recon else {
        return false;
    };

    if let Some(old_recon) = state.recon_unit {
        if old_recon != new_recon {
            ctx.tm
                .clear_unit_tasks_and_remind_available(ctx.world, old_recon, false);
        }
    }

    ctx.tm.remove_unit_tasks(ctx.world, new_recon);
    ctx.attach(header.id, new_recon);
    state.recon_unit = Some(new_recon);

    true
}

/// `TaskAttack::RequestReconUnit` (`taskattack.cpp:971`).
fn attack_request_recon_unit(
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    state: &mut super::AttackState,
    unit_type: UnitType,
    safe_distance: i32,
) -> bool {
    let team = header.team;
    let current_values = &ctx.world.team_units[team.0 as usize];

    if current_values
        .current_unit_values(unit_type as u16)
        .get_attribute(crate::unit_values::ATTRIB_SCAN)
        < safe_distance
    {
        return false;
    }

    if let Some(recon_id) = state.recon_unit {
        if let Some(recon) = ctx.unit(recon_id) {
            if recon.unit_type == unit_type {
                return true;
            }
        }

        ctx.tm
            .clear_unit_tasks_and_remind_available(ctx.world, recon_id, false);
        state.recon_unit = None;
    }

    if state.managed_unit_types.contains(&unit_type) {
        return true;
    }

    let position = attack_current_target(ctx, team, state)
        .map(|(_, target)| target.grid)
        .unwrap_or(state.site);

    let mut obtain_kind = TaskKind::obtain_units(position);
    super::create::obtain_units_add_type(&mut obtain_kind, unit_type);

    state.managed_unit_types.push(unit_type);

    ctx.append_task(
        team,
        Some(header.id),
        ctx.priority_of(header.id),
        obtain_kind,
    );

    true
}

/// `TaskAttack::IsUnitUsable` — only a spotter (`taskattack.cpp:75`).
pub(crate) fn attack_is_unit_usable(
    world: &crate::game::GameState,
    header: TaskHeader,
    state: &super::AttackState,
    unit: UnitId,
) -> bool {
    let Some(info) = world.unit(unit) else {
        return false;
    };

    if info.team != header.team || info.hits == 0 {
        return false;
    }

    state.recon_unit.is_none() && attack_is_recon_unit_usable(world, header.team, state, info)
}

/// `TaskAttack`/`TaskKillUnit`/`TaskFrontalAttack`/`TaskRetreat` —
/// `IsUnitTransferable` (`task.cpp:571`) considering the parent.
pub(crate) fn is_unit_transferable(
    world: &crate::game::GameState,
    task: &super::super::Task,
    unit: &UnitInfo,
) -> bool {
    match task.parent {
        Some(parent) => match world.ai.task_manager.task(parent) {
            Some(parent_task) => parent_task
                .kind
                .is_unit_transferable(world, parent_task, unit),
            None => true,
        },
        None => {
            let _ = unit;
            true
        }
    }
}

/// `TaskKillUnit::IsUnitUsable` (`taskkillunit.cpp:502`).
pub(crate) fn kill_unit_is_unit_usable(
    world: &crate::game::GameState,
    header: TaskHeader,
    state: &super::KillUnitState,
    unit: UnitId,
) -> bool {
    let Some(info) = world.unit(unit) else {
        return false;
    };

    if info.team != header.team || info.hits == 0 || state.spotted_unit.is_none() {
        return false;
    }

    if world.turn_counter < state.recruit_turn {
        return false;
    }

    let rounds = info
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_ROUNDS);

    if rounds <= 0 || i32::from(info.ammo) < rounds || state.managed_unit == Some(unit) {
        return false;
    }

    state.weight_table.get_weight(info.unit_type) != 0
        && i32::from(state.hits) > kill_unit_total_projected_damage(state)
}

// ---------------------------------------------------------------------------
// TaskAttackReserve
// ---------------------------------------------------------------------------

/// `TaskAttackReserve::IsUnitUsable` (`taskattackreserve.cpp:75`).
pub(crate) fn attack_reserve_is_unit_usable(
    world: &crate::game::GameState,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let Some(info) = world.unit(unit) else {
        return false;
    };

    if info.team != header.team || info.hits == 0 {
        return false;
    }

    let rounds = info
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_ROUNDS);

    // C++ also accepts factories/builders here and runs their production /
    // building branches in `AddUnit`. In this port those branches live in
    // `TaskManageBuildings` (A.3), so the reserve takes only armed mobile units;
    // otherwise it would pull a factory away from `plan_unit_production`.
    rounds > 0 && i32::from(info.ammo) >= rounds && info.speed > 0
}

/// `TaskAttackReserve::AddUnit` (`taskattackreserve.cpp:98`).
///
/// Armed, mobile units join the pool and are immediately offered to the closest
/// same-team task that can use them (a kill task of an attack, a defense
/// reserve, …). Without such a task the unit stays in the reserve, which is how
/// the AI accumulates an army before an assault.
///
/// The C++ factory/builder production branch (`taskattackreserve.cpp:136-228`)
/// stays in `TaskManageBuildings` (`plan_unit_production`, A.3), so it is not
/// duplicated here.
fn attack_reserve_add_unit(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) {
    let TaskKind::AttackReserve { units, .. } = kind else {
        return;
    };

    let Some(info) = ctx.unit(unit) else {
        return;
    };

    let rounds = info
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_ROUNDS);

    if rounds > 0 {
        if info.speed == 0 {
            ctx.tm
                .clear_unit_tasks_and_remind_available(ctx.world, unit, false);
            return;
        }

        // Find the closest same-team task that can use the unit before touching
        // the unit's task list: the handler owns a copy of the reserve state, so
        // a re-entrant `TaskAttackReserve::RemoveUnit` through the manager would
        // be overwritten when this handler's state is written back.
        let mut best_task: Option<(u32, i32)> = None;

        for task in ctx.tm.tasks() {
            if task.team != header.team
                || task.id == header.id
                || task.get_type() == super::super::task::TASK_TYPE_ATTACK_RESERVE
            {
                continue;
            }

            if !task.kind.is_unit_usable(ctx.world, task.header(), unit) {
                continue;
            }

            let bounds = ctx.tm.task_get_bounds(ctx.world, task.id);
            let dx = i32::from(info.grid.x) - bounds.ulx;
            let dy = i32::from(info.grid.y) - bounds.uly;
            let distance = dx * dx + dy * dy;

            if best_task
                .map(|(_, best_distance)| distance < best_distance)
                .unwrap_or(true)
            {
                best_task = Some((task.id, distance));
            }
        }

        if let Some((best_task, _)) = best_task {
            // `unit.RemoveTasks(); best_task->AddUnit(unit);` — the unit leaves
            // the pool (and the previous task list).
            units.retain(|id| *id != unit);
            ctx.tm.remove_unit_tasks(ctx.world, unit);
            ctx.tm.task_add_unit(ctx.world, best_task, unit);
        } else {
            if !units.contains(&unit) {
                units.push(unit);
            }

            ctx.attach(header.id, unit);
        }
    }
}

/// `TaskAttackReserve::EndTurn` (`taskattackreserve.cpp:234`) — reload, repair
/// and upgrade the pool members, then let them report their movement.
pub(crate) fn attack_reserve_end_turn(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
) {
    let TaskKind::AttackReserve { units, .. } = kind else {
        return;
    };

    let opponent = ctx.world.ai.settings.opponent;
    let mut scheduled = Vec::new();

    for unit_id in units.clone() {
        if ctx.tm.unit_task(unit_id) != Some(header.id) {
            continue;
        }

        let Some(info) = ctx.unit(unit_id) else {
            continue;
        };

        let rounds = info
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_ROUNDS);
        let base_ammo = info
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_AMMO);
        let base_hits = info
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_HITS);

        if i32::from(info.ammo) < rounds
            || (info.flags & unit_flags::STATIONARY != 0
                && i32::from(info.ammo) < base_ammo / 2)
        {
            scheduled.push(TaskKind::reload(unit_id));
        } else if i32::from(info.hits) < base_hits / 2
            && opponent >= Opponent::Apprentice as u8
        {
            scheduled.push(TaskKind::repair(unit_id));
        } else if info.flags
            & (unit_flags::MOBILE_AIR_UNIT
                | unit_flags::MOBILE_SEA_UNIT
                | unit_flags::MOBILE_LAND_UNIT)
            != 0
            && info.flags & unit_flags::REGENERATING_UNIT == 0
            && opponent >= Opponent::Expert as u8
            && ctx.world.ai.players[header.team.0 as usize]
                .should_upgrade_unit(ctx.world, &info)
        {
            scheduled.push(TaskKind::upgrade(unit_id));
        }
    }

    for task_kind in scheduled {
        ctx.append_task(header.team, None, TASK_PRIORITY_REPAIR, task_kind);
    }

    for unit_id in units.clone() {
        let moves = ctx.unit(unit_id).map(|info| info.speed > 0).unwrap_or(false);
        let ready = ctx.ready_to_take_orders(unit_id) && ctx.tm.unit_task(unit_id) == Some(header.id);

        if moves && ready {
            ctx.remind_move_finished(unit_id, false);
        }
    }
}

/// `TaskAttackReserve::Execute` (`taskattackreserve.cpp:267`).
fn attack_reserve_execute(
    _kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let Some(info) = ctx.unit(unit) else {
        return false;
    };

    if info.speed == 0 || !ctx.ready_to_take_orders(unit) {
        return false;
    }

    if attack::evaluate_attack(ctx.tm, ctx.world, unit, true) {
        true
    } else {
        attack_reserve_chase_attacker(ctx, header, unit)
    }
}

/// `AiAttack_GetAttackTask` (`aiattack.cpp:1088`) — the attack task behind a
/// unit, walking up through its movement/kill tasks.
fn attack_task_of(ctx: &TaskCtx<'_>, unit: UnitId) -> Option<u32> {
    let mut task_id = ctx.tm.unit_task(unit)?;

    loop {
        let task = ctx.tm.task(task_id)?;

        match task.get_type() {
            super::super::task::TASK_TYPE_ATTACK => return Some(task_id),
            super::super::task::TASK_TYPE_MOVE
            | super::super::task::TASK_TYPE_FIND_PATH
            | super::super::task::TASK_TYPE_KILL_UNIT => {
                task_id = task.parent?;
            }
            _ => return None,
        }
    }
}

/// `AiAttack_FollowAttacker` (`aiattack.cpp:1113`) — the reserve follows the
/// closest attacking unit; without an attack it goes home.
fn attack_reserve_chase_attacker(
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let Some(unit_info) = ctx.unit(unit) else {
        return false;
    };

    let mut leader: Option<UnitId> = None;
    let mut minimum_distance = i32::MAX;
    let mut leader_in_execution = false;

    for list_kind in [UnitListKind::MobileLandSea, UnitListKind::MobileAir] {
        for other_id in ctx.list(list_kind) {
            let Some(other) = ctx.unit(other_id) else {
                continue;
            };

            if other.team != header.team {
                continue;
            }

            let Some(attack_id) = attack_task_of(ctx, other_id) else {
                continue;
            };

            let Some(task) = ctx.tm.task(attack_id) else {
                continue;
            };

            if task.compare_priority(ctx.world, TASK_PRIORITY_FOLLOW_ATTACK) > 0 {
                continue;
            }

            let in_execution = matches!(
                &task.kind,
                TaskKind::Attack(state) if state.op_state >= attack_state::ADVANCE
            );

            let distance = crate::access::squared_distance(unit_info.grid, other.grid);

            let replace = leader.is_none()
                || (!leader_in_execution && in_execution)
                || (leader_in_execution == in_execution && distance < minimum_distance);

            if replace {
                leader = Some(other_id);
                minimum_distance = distance;
                leader_in_execution = in_execution;
            }
        }
    }

    if let Some(leader_id) = leader {
        let target_position = attack_task_of(ctx, leader_id)
            .and_then(|attack_id| ctx.tm.task(attack_id))
            .and_then(|task| match &task.kind {
                TaskKind::Attack(state) => state.kill_unit_task,
                _ => None,
            })
            .and_then(|kill_id| ctx.tm.task(kill_id))
            .and_then(|task| match &task.kind {
                TaskKind::KillUnit(kill) => Some(kill.target_position),
                _ => None,
            })
            .or_else(|| ctx.unit(leader_id).map(|info| info.grid))
            .unwrap_or(unit_info.grid);

        let range = unit_info
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_RANGE);

        let destination = attack::choose_site_for_attacker(
            ctx.world,
            &unit_info,
            target_position,
            CautionLevel::AvoidNextTurnsFire,
            range,
            true,
        )
        .map(|(site, _)| site)
        .unwrap_or(target_position);

        if destination != unit_info.grid {
            let move_kind =
                TaskKind::move_to(unit, destination, 0, CautionLevel::AvoidNextTurnsFire);
            ctx.append_task(
                header.team,
                Some(header.id),
                TASK_PRIORITY_FOLLOW_ATTACK,
                move_kind,
            );
        }

        return true;
    }

    // `TaskMoveHome(unit, this)` — no attacker to follow.
    let move_home = TaskKind::MoveHome {
        unit: Some(unit),
        site: Point::ZERO,
    };
    ctx.append_task(
        header.team,
        Some(header.id),
        TASK_PRIORITY_FOLLOW_ATTACK,
        move_home,
    );

    true
}

/// `TaskDefenseReserve::IsUnitUsable`.
pub(crate) fn defense_reserve_is_unit_usable(
    world: &crate::game::GameState,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let Some(info) = world.unit(unit) else {
        return false;
    };

    info.team == header.team
        && info.hits > 0
        && info.ammo > 0
        && (info.is_ground() || info.is_amphibious())
}

/// `TaskSupportAttack::IsUnitUsable` (`unit_flags` — the support class).
pub(crate) fn support_attack_is_unit_usable(
    world: &crate::game::GameState,
    header: TaskHeader,
    kind: &TaskKind,
    unit: UnitId,
) -> bool {
    let TaskKind::SupportAttack { unit_flags, .. } = kind else {
        return false;
    };

    let Some(info) = world.unit(unit) else {
        return false;
    };

    info.team == header.team && info.hits > 0 && info.flags & *unit_flags != 0
}

// ---------------------------------------------------------------------------
// TaskFrontalAttack
// ---------------------------------------------------------------------------

fn frontal_attack_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::FrontalAttack {
        units1,
        units2,
        site,
        movement_in_progress,
        ..
    } = kind
    else {
        return;
    };

    units1.retain(|unit| ctx.unit(*unit).map(|info| info.hits > 0).unwrap_or(false));
    units2.retain(|unit| ctx.unit(*unit).map(|info| info.hits > 0).unwrap_or(false));

    if units1.is_empty() && units2.is_empty() {
        ctx.remove_self(header.id);
        return;
    }

    if *movement_in_progress {
        return;
    }

    *movement_in_progress = true;

    for unit_id in units1.clone() {
        if let Some(info) = ctx.unit(unit_id) {
            if info.orders == UnitOrder::Await {
                let priority = ctx.priority_of(header.id);
                let move_kind =
                    TaskKind::move_to(unit_id, *site, 0, CautionLevel::AvoidNextTurnsFire);
                ctx.append_task(info.team, Some(header.id), priority, move_kind);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TaskWaitToAttack
// ---------------------------------------------------------------------------

/// `TaskWaitToAttack::Init` (`taskwaittoattack.cpp:41`) — only one wait task per
/// unit; a duplicate removes itself.
fn wait_to_attack_init(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    let TaskKind::WaitToAttack { attacker } = kind else {
        return false;
    };

    let Some(unit) = *attacker else {
        ctx.remove_self(header.id);
        return true;
    };

    let already_waiting = ctx.tm.unit_tasks_of(unit).into_iter().any(|task_id| {
        ctx.tm
            .task(task_id)
            .map(|task| task.get_type() == super::super::task::TASK_TYPE_WAIT_TO_ATTACK)
            .unwrap_or(false)
    });

    if already_waiting {
        ctx.set_parent(header.id, None);
        ctx.remove_self(header.id);
    } else {
        ctx.attach(header.id, unit);
    }

    true
}

/// `TaskWaitToAttack::CheckReactions` (`taskwaittoattack.cpp:71`).
fn wait_to_attack_run(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::WaitToAttack { attacker } = kind else {
        return;
    };

    let Some(unit_id) = *attacker else {
        ctx.remove_self(header.id);
        return;
    };

    let Some(info) = ctx.unit(unit_id) else {
        ctx.remove_self(header.id);
        return;
    };

    if info.hits == 0 || info.shots == 0 || info.ammo == 0 {
        ctx.remove_self(header.id);
        return;
    }

    let reactions_settled = info.delayed_reaction == 0
        && ctx.world.pending_attacks.is_empty()
        && ctx.world.ai.reaction_state != super::super::AiReactionState::AnimationsActive;

    if reactions_settled {
        attack::evaluate_attack(ctx.tm, ctx.world, unit_id, true);

        ctx.remove_self(header.id);
    }
}

// ---------------------------------------------------------------------------
// TaskEscort
// ---------------------------------------------------------------------------

fn escort_end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::Escort {
        target,
        escort,
        unit_type,
        is_escort_requested,
    } = kind
    else {
        return;
    };

    let Some(target_id) = *target else {
        ctx.remove_self(header.id);
        return;
    };

    let Some(target_info) = ctx.unit(target_id) else {
        ctx.remove_self(header.id);
        return;
    };

    // Find an escort of the required type.
    if escort.is_none() {
        let candidate = ctx
            .list(UnitListKind::MobileLandSea)
            .into_iter()
            .chain(ctx.list(UnitListKind::MobileAir))
            .find(|unit_id| {
                ctx.unit(*unit_id)
                    .map(|info| {
                        info.team == header.team && info.unit_type == *unit_type && info.hits > 0
                    })
                    .unwrap_or(false)
            });

        if let Some(unit_id) = candidate {
            *escort = Some(unit_id);
            ctx.attach(header.id, unit_id);
        } else if !*is_escort_requested {
            *is_escort_requested = true;

            let mut obtain = TaskKind::obtain_units(target_info.grid);
            super::create::obtain_units_add_type(&mut obtain, *unit_type);
            ctx.append_task(header.team, Some(header.id), TASK_PRIORITY_ESCORT, obtain);
        }

        return;
    }

    let Some(escort_id) = *escort else { return };

    if ctx
        .unit(escort_id)
        .map(|info| info.hits == 0)
        .unwrap_or(true)
    {
        *escort = None;
        return;
    }

    if !ctx.ready_to_take_orders(escort_id) {
        return;
    }

    // `ORDER_MOVE_TO_UNIT` toward the target.
    if let Some(mut info) = ctx.unit(escort_id) {
        info.parent_unit = Some(target_id);
        ctx.write_unit(escort_id, info);
    }

    ctx.set_new_order(
        escort_id,
        UnitOrder::MoveToUnit,
        UnitOrderState::ExecutingOrder,
    );
}

// ---------------------------------------------------------------------------
// TaskRetreat
// ---------------------------------------------------------------------------

/// `TaskRetreat::Search` — a safe cell within the radius.
///
/// C++ iterates cells with a `ZoneWalker`-like loop using `steps_in_direction`
/// and `direction`; here — the same `ZoneWalker` walk from the current position,
/// the criterion is the AI damage map (`GetDamagePotentialMap`) and passability.
fn retreat_search(ctx: &TaskCtx<'_>, state: &super::RetreatState) -> Option<Point> {
    let unit_id = state.unit_to_retreat?;
    let info = ctx.unit(unit_id)?;
    let caution = state.caution_level;

    let map = crate::ai::strategy::get_damage_potential_map(ctx.world, info.team, &info, caution);

    let mut walker =
        crate::ai::threat::ZoneWalker::new(info.grid, i32::from(state.search_radius.max(1)));

    loop {
        let site = walker.current_location();

        if site != info.grid && crate::ai::threat::in_bounds(site) {
            let safe = match &map {
                Some(map) => {
                    map.damage_at(i32::from(site.x), i32::from(site.y)) < i32::from(info.hits)
                }
                None => true,
            };

            if safe
                && crate::access::is_accessible(
                    ctx.world,
                    info.unit_type,
                    info.team,
                    site,
                    crate::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
                ) != 0
            {
                return Some(site);
            }
        }

        if !walker.find_next() {
            break;
        }
    }

    None
}

fn retreat_run(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let (unit_id, found_valid_position, destination, caution_level) = match kind {
        TaskKind::Retreat {
            unit_to_retreat,
            found_valid_position,
            destination,
            caution_level,
            ..
        } => (
            *unit_to_retreat,
            found_valid_position,
            destination,
            *caution_level,
        ),
        _ => return,
    };

    let Some(unit_id) = unit_id else {
        ctx.remove_self(header.id);
        return;
    };

    let Some(info) = ctx.unit(unit_id) else {
        ctx.remove_self(header.id);
        return;
    };

    if info.hits == 0 || info.speed == 0 {
        ctx.remove_self(header.id);
        return;
    }

    let snapshot = super::RetreatState {
        unit_to_retreat: Some(unit_id),
        search_radius: 3,
        position: info.grid,
        direction: 0,
        steps_in_direction: 0,
        caution_level,
        found_valid_position: *found_valid_position,
        destination: *destination,
    };

    match retreat_search(ctx, &snapshot) {
        Some(site) => {
            *destination = site;
            *found_valid_position = 1;

            if info.orders == UnitOrder::Await || info.orders == UnitOrder::Sentry {
                if let Some(mut info) = ctx.unit(unit_id) {
                    info.ai_state_bits |= crate::enums::AI_STATE_NO_RETREAT;
                    ctx.write_unit(unit_id, info);
                }

                let priority = ctx.priority_of(header.id).max(TASK_PRIORITY_EMERGENCY);
                let move_kind = TaskKind::move_to(unit_id, site, 0, CautionLevel::AvoidAllDamage);
                ctx.append_task(info.team, Some(header.id), priority, move_kind);
            }
        }
        None => {
            if let Some(mut info) = ctx.unit(unit_id) {
                info.ai_state_bits |= crate::enums::AI_STATE_NO_RETREAT;
                ctx.write_unit(unit_id, info);
            }

            ctx.remove_self(header.id);
        }
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// `Init` for combat classes.
pub fn init(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::Attack(state) => {
            // `TaskAttack::Init` (`taskattack.cpp:271`).
            state.access_flags = unit_flags::MOBILE_AIR_UNIT
                | unit_flags::MOBILE_SEA_UNIT
                | unit_flags::MOBILE_LAND_UNIT;
            let _ = (ctx, header);
            true
        }
        TaskKind::KillUnit(_) => true,
        TaskKind::FrontalAttack { .. } => true,
        TaskKind::WaitToAttack { .. } => wait_to_attack_init(kind, ctx, header),
        TaskKind::Retreat { .. } => {
            retreat_run(kind, ctx, header);
            true
        }
        TaskKind::SupportAttack { .. } => true,
        TaskKind::AttackReserve { .. }
        | TaskKind::DefenseReserve { .. }
        | TaskKind::Escort { .. } => true,
        _ => false,
    }
}

/// `BeginTurn` for combat classes.
pub fn begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::Attack(_) => {
            attack_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::KillUnit(_) => {
            kill_unit_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::FrontalAttack { .. } => {
            frontal_attack_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::WaitToAttack { .. } => {
            wait_to_attack_run(kind, ctx, header);
            true
        }
        TaskKind::Retreat { .. } => {
            retreat_run(kind, ctx, header);
            true
        }
        TaskKind::SupportAttack { .. } => {
            support_attack_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::AttackReserve { .. } => {
            // `TaskAttackReserve` has no `BeginTurn`: `Task::BeginTurn` calls
            // `EndTurn` (`task.cpp:631`), which reloads/repairs the pool.
            attack_reserve_end_turn(kind, ctx, header);
            true
        }
        TaskKind::DefenseReserve { .. } => {
            defense_reserve_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::Escort { .. } => {
            escort_end_turn(kind, ctx, header);
            true
        }
        _ => false,
    }
}

/// `EndTurn` for combat classes.
pub fn end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::Attack(_) => {
            attack_end_turn(kind, ctx, header);
            true
        }
        TaskKind::KillUnit(_) => {
            kill_unit_end_turn(kind, ctx, header);
            true
        }
        // `Task::BeginTurn()` calls `EndTurn()` (`task.cpp:631`), so classes
        // without their own `BeginTurn` override (WaitToAttack, Retreat,
        // Escort) use their turn-end helper here. The `EndTurn`
        // implementations of FrontalAttack/SupportAttack/AttackReserve/
        // DefenseReserve are not ported yet; their reduced begin-turn helpers
        // are reused as before.
        TaskKind::FrontalAttack { .. } => {
            frontal_attack_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::WaitToAttack { .. } => {
            wait_to_attack_run(kind, ctx, header);
            true
        }
        TaskKind::Retreat { .. } => {
            retreat_run(kind, ctx, header);
            true
        }
        TaskKind::SupportAttack { .. } => {
            support_attack_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::AttackReserve { .. } => {
            attack_reserve_end_turn(kind, ctx, header);
            true
        }
        TaskKind::DefenseReserve { .. } => {
            defense_reserve_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::Escort { .. } => {
            escort_end_turn(kind, ctx, header);
            true
        }
        _ => false,
    }
}

/// `TaskSupportAttack::BeginTurn` — gather support units.
fn support_attack_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::SupportAttack {
        units, unit_flags, ..
    } = kind
    else {
        return;
    };

    units.retain(|unit| ctx.unit(*unit).map(|info| info.hits > 0).unwrap_or(false));

    for unit_id in ctx.list(UnitListKind::MobileLandSea) {
        if units.contains(&unit_id) {
            continue;
        }

        let Some(info) = ctx.unit(unit_id) else {
            continue;
        };
        if info.team != header.team || info.hits == 0 || info.flags & *unit_flags == 0 {
            continue;
        }

        if crate::ai::task::task_is_recruitable(
            ctx.tm,
            ctx.world,
            unit_id,
            header.team,
            ctx.priority_of(header.id),
        ) {
            units.push(unit_id);
            ctx.attach(header.id, unit_id);
        }
    }

    if units.is_empty() {
        ctx.remove_self(header.id);
        return;
    }
}

/// `TaskDefenseReserve::BeginTurn` — the defense reserve at the site.
fn defense_reserve_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::DefenseReserve { units, site } = kind else {
        return;
    };

    units.retain(|unit| ctx.unit(*unit).map(|info| info.hits > 0).unwrap_or(false));

    if units.is_empty() {
        // Recruiting: the first suitable team unit.
        for unit_id in ctx.list(UnitListKind::MobileLandSea) {
            if defense_reserve_is_unit_usable(ctx.world, header, unit_id)
                && crate::ai::task::task_is_recruitable(
                    ctx.tm,
                    ctx.world,
                    unit_id,
                    header.team,
                    ctx.priority_of(header.id),
                )
            {
                *units = vec![unit_id];
                ctx.attach(header.id, unit_id);
                break;
            }
        }

        if units.is_empty() {
            ctx.remove_self(header.id);
            return;
        }
    }

    for unit_id in units.clone() {
        let Some(info) = ctx.unit(unit_id) else {
            continue;
        };
        if info.orders != UnitOrder::Await && info.orders != UnitOrder::Sentry {
            continue;
        }

        let dx = i32::from(info.grid.x) - i32::from(site.x);
        let dy = i32::from(info.grid.y) - i32::from(site.y);

        if dx * dx + dy * dy > 9 {
            let priority = ctx.priority_of(header.id).max(TASK_PRIORITY_FOLLOW_DEFENSE);
            let move_kind = TaskKind::move_to(unit_id, *site, 9, CautionLevel::AvoidNextTurnsFire);
            ctx.append_task(info.team, Some(header.id), priority, move_kind);
        }
    }
}

/// `Execute(unit)` for combat classes.
pub fn execute(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    match kind {
        TaskKind::KillUnit(_) => kill_unit_execute(kind, ctx, header, unit),
        TaskKind::Attack(_) => {
            let Some(kill_task) = (match kind {
                TaskKind::Attack(state) => state.kill_unit_task,
                _ => None,
            }) else {
                return false;
            };

            ctx.tm.task_execute(ctx.world, kill_task, unit)
        }
        TaskKind::WaitToAttack { .. } => false,
        TaskKind::AttackReserve { .. } => attack_reserve_execute(kind, ctx, header, unit),
        TaskKind::DefenseReserve { .. } => {
            defense_reserve_begin_turn(kind, ctx, header);
            true
        }
        TaskKind::Escort { .. } => {
            escort_end_turn(kind, ctx, header);
            true
        }
        _ => false,
    }
}

/// `AddUnit(unit)` for combat classes.
pub fn add_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    match kind {
        TaskKind::Attack(_) => attack_add_unit(kind, ctx, header, unit),
        TaskKind::KillUnit(_) => kill_unit_add_unit(kind, ctx, header, unit),
        TaskKind::FrontalAttack { units1, .. } => {
            if !units1.contains(&unit) {
                units1.push(unit);
                ctx.attach(header.id, unit);
            }
        }
        TaskKind::SupportAttack { units, .. } => {
            if !units.contains(&unit) {
                units.push(unit);
                ctx.attach(header.id, unit);
            }
        }
        TaskKind::AttackReserve { .. } => attack_reserve_add_unit(kind, ctx, header, unit),
        TaskKind::DefenseReserve { units, .. } => {
            if !units.contains(&unit) {
                units.push(unit);
                ctx.attach(header.id, unit);
            }
        }
        TaskKind::Escort { escort, .. } => {
            if escort.is_none() {
                *escort = Some(unit);
                ctx.attach(header.id, unit);
            }
        }
        _ => {}
    }
}

/// `RemoveUnit(unit)` for combat classes.
pub fn remove_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    match kind {
        TaskKind::KillUnit(_) => kill_unit_remove_unit(kind, ctx, header, unit),
        TaskKind::Attack(_) => {}
        TaskKind::FrontalAttack { units1, units2, .. } => {
            units1.retain(|id| *id != unit);
            units2.retain(|id| *id != unit);

            if units1.is_empty() && units2.is_empty() {
                ctx.remove_self(header.id);
            }
        }
        TaskKind::SupportAttack { units, .. } | TaskKind::DefenseReserve { units, .. } => {
            units.retain(|id| *id != unit);

            if units.is_empty() {
                ctx.remove_self(header.id);
            }
        }
        TaskKind::AttackReserve {
            units, total_worth, ..
        } => {
            // `TaskAttackReserve::RemoveUnit` — the reserve stays alive.
            if let Some(info) = ctx.unit(unit) {
                *total_worth -= super::super::strategy::normal_rate_build_cost(
                    ctx.world,
                    header.team,
                    info.unit_type,
                );
            }

            units.retain(|id| *id != unit);
        }
        TaskKind::Escort { escort, target, .. } => {
            if *escort == Some(unit) || *target == Some(unit) {
                ctx.remove_self(header.id);
            }
        }
        TaskKind::WaitToAttack { attacker } => {
            if *attacker == Some(unit) {
                ctx.remove_self(header.id);
            }
        }
        TaskKind::Retreat {
            unit_to_retreat, ..
        } => {
            if *unit_to_retreat == Some(unit) {
                ctx.remove_self(header.id);
            }
        }
        _ => {}
    }
}

/// `RemoveSelf()` for combat classes.
pub fn remove_self(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::Attack(state) => {
            for task_id in state.secondary_targets.clone() {
                attack_remove_kill_task(ctx, header, state, task_id);
            }

            if let Some(recon) = state.recon_unit {
                ctx.tm
                    .clear_unit_tasks_and_remind_available(ctx.world, recon, false);
                state.recon_unit = None;
            }

            state.primary_targets.clear();
            state.secondary_targets.clear();
            state.kill_unit_task = None;

            ctx.set_parent(header.id, None);
            ctx.remove_self(header.id);
            true
        }
        TaskKind::KillUnit(state) => {
            let parent = header.parent;
            let units = state.units.clone();

            // `TaskKillUnit::RemoveSelf`: release the members, then notify the
            // parent. Removing the task first prevents the unit cleanup from
            // re-entering this task's `RemoveUnit`.
            ctx.tm.remove_task(ctx.world, header.id);

            for unit in units {
                ctx.tm
                    .clear_unit_tasks_and_remind_available(ctx.world, unit, false);
            }

            if let Some(parent) = parent {
                if ctx.tm.has_task(parent) {
                    ctx.tm.task_child_complete(ctx.world, parent, header.id);
                }
            }

            true
        }
        TaskKind::AttackReserve { units, .. } => {
            for unit in units.clone() {
                ctx.tm
                    .clear_unit_tasks_and_remind_available(ctx.world, unit, true);
            }

            units.clear();
            ctx.set_parent(header.id, None);
            ctx.remove_self(header.id);
            true
        }
        TaskKind::FrontalAttack { .. }
        | TaskKind::WaitToAttack { .. }
        | TaskKind::SupportAttack { .. }
        | TaskKind::DefenseReserve { .. }
        | TaskKind::Escort { .. }
        | TaskKind::Retreat { .. } => {
            ctx.set_parent(header.id, None);
            ctx.remove_self(header.id);
            true
        }
        _ => false,
    }
}

/// `ChildComplete(task)` for combat classes.
pub fn child_complete(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    child: u32,
) -> bool {
    match kind {
        TaskKind::Attack(state) => {
            state.primary_targets.retain(|task| *task != child);
            state.secondary_targets.retain(|task| *task != child);

            if state.kill_unit_task == Some(child) {
                state.kill_unit_task = None;
            }

            if state.secondary_targets.is_empty() {
                ctx.remove_self(header.id);
            } else if state.kill_unit_task.is_none() {
                // `TaskAttack::ChildComplete` (`taskattack.cpp:308`): pick the
                // next target of the group.
                attack_choose_first_target(ctx, state);
            }

            true
        }
        TaskKind::KillUnit(_) => {
            kill_unit_child_complete(kind, ctx, header, child);
            true
        }
        TaskKind::FrontalAttack { .. }
        | TaskKind::SupportAttack { .. }
        | TaskKind::AttackReserve { .. }
        | TaskKind::DefenseReserve { .. }
        | TaskKind::Escort { .. } => {
            let _ = (ctx, header, child);
            true
        }
        _ => false,
    }
}

/// Notification about a child `TaskMove` completion.
pub fn move_finished(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
    result: i8,
) -> bool {
    match kind {
        TaskKind::KillUnit(_) => {
            kill_unit_execute(kind, ctx, header, unit);
            let _ = result;
            true
        }
        TaskKind::Attack(_) => {
            let kill_task = match kind {
                TaskKind::Attack(state) => state.kill_unit_task,
                _ => None,
            };

            if let Some(kill_task) = kill_task {
                ctx.tm
                    .task_move_finished(ctx.world, kill_task, unit, result);
            }

            true
        }
        TaskKind::FrontalAttack {
            movement_in_progress,
            ..
        } => {
            *movement_in_progress = false;
            false
        }
        TaskKind::Retreat { .. } => {
            if result == crate::ai::task::TASKMOVE_RESULT_BLOCKED
                || result == crate::ai::task::TASKMOVE_RESULT_CANCELLED
            {
                ctx.remove_self(header.id);
            }
            true
        }
        TaskKind::SupportAttack { .. }
        | TaskKind::AttackReserve { .. }
        | TaskKind::DefenseReserve { .. }
        | TaskKind::Escort { .. } => {
            let _ = (ctx, header, unit, result);
            true
        }
        _ => false,
    }
}

/// `CheckReactions` — `TaskWaitToAttack`/`TaskKillUnit`.
pub fn check_reactions(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::WaitToAttack { .. } => {
            wait_to_attack_run(kind, ctx, header);
            false
        }
        TaskKind::KillUnit(_) => false,
        _ => false,
    }
}

/// `ExchangeOperator` — `TaskEscort` (swapping the escort).
pub fn exchange_operator(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    let TaskKind::Escort {
        escort, unit_type, ..
    } = kind
    else {
        return false;
    };

    let Some(current) = *escort else {
        return false;
    };

    let Some(candidate) = ctx.unit(unit) else {
        return false;
    };

    if candidate.unit_type != *unit_type || candidate.team != header.team {
        return false;
    }

    let current_distance = ctx
        .unit(current)
        .map(|info| i32::from(info.hits))
        .unwrap_or(0);

    if i32::from(candidate.hits) <= current_distance {
        return false;
    }

    ctx.clear_unit_tasks(current, false);
    *escort = Some(unit);
    ctx.attach(header.id, unit);

    true
}

/// `EventUnitDestroyed(unit)` for combat classes.
pub fn event_unit_destroyed(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    match kind {
        TaskKind::KillUnit(_) => kill_unit_event_unit_destroyed(kind, ctx, header, unit),
        TaskKind::Attack(_) => false,
        TaskKind::FrontalAttack { units1, units2, .. } => {
            units1.retain(|id| *id != unit);
            units2.retain(|id| *id != unit);

            if units1.is_empty() && units2.is_empty() {
                ctx.remove_self(header.id);
            }

            true
        }
        TaskKind::SupportAttack { units, .. } | TaskKind::DefenseReserve { units, .. } => {
            units.retain(|id| *id != unit);
            if units.is_empty() {
                ctx.remove_self(header.id);
            }
            true
        }
        TaskKind::AttackReserve { units, .. } => {
            // The reserve pool outlives its members.
            units.retain(|id| *id != unit);
            true
        }
        TaskKind::Escort { escort, target, .. } => {
            if *escort == Some(unit) || *target == Some(unit) {
                ctx.remove_self(header.id);
            }
            true
        }
        TaskKind::WaitToAttack { attacker } => {
            if *attacker == Some(unit) {
                ctx.remove_self(header.id);
            }
            true
        }
        _ => false,
    }
}

/// `EventEnemyUnitSpotted(unit)` for combat classes.
pub fn event_enemy_unit_spotted(
    kind: &mut TaskKind,
    ctx: &mut TaskCtx<'_>,
    header: TaskHeader,
    unit: UnitId,
) -> bool {
    match kind {
        TaskKind::KillUnit(_) => kill_unit_event_enemy_spotted(kind, ctx, header, unit),
        TaskKind::Attack(_) => false,
        _ => false,
    }
}

/// `EventCargoTransfer` — not overridden.
pub fn event_cargo_transfer(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit: UnitId,
) -> bool {
    false
}

/// `EventUnitLoaded` — `TaskEscort`/`TaskSupportAttack` (structurally).
pub fn event_unit_loaded(
    kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit1: UnitId,
    _unit2: UnitId,
) -> bool {
    matches!(
        kind,
        TaskKind::Escort { .. } | TaskKind::SupportAttack { .. }
    )
}

/// `EventUnitUnloaded` — structurally.
pub fn event_unit_unloaded(
    kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit1: UnitId,
    _unit2: UnitId,
) -> bool {
    matches!(
        kind,
        TaskKind::Escort { .. } | TaskKind::SupportAttack { .. }
    )
}

/// `TaskAttack::GetCautionLevel` (`taskattack.cpp:77`) — depends on the unit's
/// role in the group and on the attack phase.
pub fn attack_caution_level(
    world: &crate::game::GameState,
    state: &super::AttackState,
    unit: &UnitInfo,
) -> CautionLevel {
    let move_and_fire = unit
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_MOVE_AND_FIRE);

    if move_and_fire != 0 && world.ai.settings.opponent >= Opponent::Average as u8 {
        if unit.shots > 0 {
            CautionLevel::AvoidReactionFire
        } else {
            CautionLevel::AvoidAllDamage
        }
    } else if unit.ammo > 0 || state.op_state >= attack_state::BOLD_SEARCH {
        CautionLevel::AvoidNextTurnsFire
    } else {
        CautionLevel::AvoidAllDamage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{PlayMode, TeamClan};
    use crate::geo::Point;
    use crate::team::{TeamId, TeamKind, TeamUnits};
    use crate::unit_values::{ATTRIB_ATTACK, ATTRIB_RANGE, ATTRIB_ROUNDS, ATTRIB_SPEED};
    use crate::GameState;

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
        crate::ai_unit_spotted(state, unit, team);
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
        crate::ai_unit_spotted(&mut world, target, TeamId::RED);

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
}
