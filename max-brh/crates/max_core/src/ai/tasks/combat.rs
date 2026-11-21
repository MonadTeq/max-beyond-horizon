//! Бой: `TaskAttack`, `TaskKillUnit`, `TaskFrontalAttack`, `TaskWaitToAttack`,
//! `TaskSupportAttack`, `TaskAttackReserve`, `TaskDefenseReserve`,
//! `TaskEscort`, `TaskRetreat`.
//!
//! Оракул: `taskattack.cpp/.hpp`, `taskkillunit.cpp/.hpp`,
//! `taskfrontalattack.cpp/.hpp`, `taskwaittoattack.cpp/.hpp`,
//! `tasksupportattack.cpp/.hpp`, `taskattackreserve.cpp/.hpp`,
//! `taskdefensereserve.cpp/.hpp`, `taskescort.cpp/.hpp`,
//! `taskretreat.cpp/.hpp`, `aiattack.cpp` (решения об атаке — модуль
//! [`crate::ai::attack`]).
//!
//! # Что портировано
//!
//! * `TaskAttack`: `target_team`/`op_state`/`primary_targets`/
//!   `secondary_targets`, `Init`, `BeginTurn`, `EndTurn`, `AddUnit`,
//!   `IsUnitUsable`, `Execute`, `RemoveSelf`, `RemoveUnit`, `ChildComplete`,
//!   `ExchangeOperator` (структурно), `GetCautionLevel`.
//! * `TaskKillUnit`: `spotted_unit`, `units`, `unit_damages`,
//!   `required_damage`, `weight_table`, `seek_target`, `recruit_turn`;
//!   `GetProjectedDamage`, `IsUnitSuitable`, `BeginTurn`, `EndTurn`,
//!   `Execute`, `EventUnitDestroyed`, `EventEnemyUnitSpotted`,
//!   `HoldRecruitsThisTurn`.
//! * `TaskWaitToAttack`: `Init`, `EndTurn`, `CheckReactions`, `IsThinking`.
//! * `TaskRetreat`: поиск безопасной клетки по карте урона
//!   (`Ai_IsDangerousLocation`/`GetDamagePotentialMap`) и `TaskMove` к ней.
//! * `TaskFrontalAttack`, `TaskSupportAttack`, `TaskAttackReserve`,
//!   `TaskDefenseReserve`, `TaskEscort` — цикл жизни и приказы; их внутренние
//!   эвристики (транспортные карты, сектора обороны, `TransporterMap`) сведены
//!   к детерминированному выбору сайта/юнита (см. примечания в функциях).

use crate::enums::{CautionLevel, UnitOrder, UnitOrderState, UnitType};
use crate::geo::Point;
use crate::team::TeamId;
use crate::unit::{UnitId, UnitInfo, UnitListKind};

use super::super::attack;
use super::super::task::{
    TaskHeader, TASK_PRIORITY_ATTACK_DEFAULT, TASK_PRIORITY_ATTACK_RESERVE_POOL,
    TASK_PRIORITY_EMERGENCY, TASK_PRIORITY_ESCORT, TASK_PRIORITY_FOLLOW_DEFENSE,
};
use super::{attack_state, TaskCtx, TaskKind, UnitDamage};

// ---------------------------------------------------------------------------
// Общие помощники
// ---------------------------------------------------------------------------

/// Цель задачи `TaskKillUnit` по индексу `SpottedUnit` (`AiPlayer::spotted_units`).
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

/// Цель `SpottedUnit` без `TaskCtx` (для `aiattack.cpp`-слоя).
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

/// `TaskKillUnit::GetProjectedDamage` (статическая) — суммарный урон группы.
pub(crate) fn kill_unit_total_projected_damage(state: &super::KillUnitState) -> i32 {
    state.unit_damages.iter().map(|entry| entry.damage).sum()
}

/// `TaskKillUnit::GetProjectedDamage(attacker, target)` — ожидаемый урон.
pub fn get_projected_damage(
    _world: &crate::game::GameState,
    attacker: &UnitInfo,
    target: &UnitInfo,
) -> i32 {
    let rounds = attacker
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_ROUNDS);
    let shots = if i32::from(attacker.shots) > 0 {
        i32::from(attacker.shots)
    } else {
        rounds
    };

    let per_shot = crate::combat::get_attack_damage(attacker, target, 0);

    per_shot * shots
}

/// `TaskKillUnit::IsUnitSuitable` — юнит может бить цель.
pub(crate) fn is_unit_suitable(
    ctx: &TaskCtx<'_>,
    team: TeamId,
    state: &super::KillUnitState,
    unit: UnitId,
) -> bool {
    let Some(info) = ctx.unit(unit) else {
        return false;
    };

    if info.team != team || info.hits == 0 || info.ammo == 0 {
        return false;
    }

    let Some(target_type) = state.target_type else {
        return false;
    };

    crate::access::is_valid_attack_target_type(info.unit_type, target_type)
}

/// `TaskKillUnit::IsAttackShareLeft` — доля военной силы ещё не исчерпана.
fn is_attack_share_left(ctx: &TaskCtx<'_>, team: TeamId, state: &super::KillUnitState) -> bool {
    let share = ctx.world.ai.players[team.0 as usize].attack_value_share;
    let total = kill_unit_total_projected_damage(state);

    total < share || state.units.is_empty()
}

/// `TaskKillUnit::UpdateDemand` — пересчитать требуемый урон и вклады юнитов.
fn kill_unit_update_demand(ctx: &mut TaskCtx<'_>, team: TeamId, state: &mut super::KillUnitState) {
    let Some((target_id, position, target_type)) =
        spotted_target(ctx, team, state.spotted_unit.unwrap_or(0))
    else {
        state.required_damage = u16::MAX;
        return;
    };

    state.target_position = position;
    state.target_type = Some(target_type);

    let Some(target) = ctx.unit(target_id) else {
        state.required_damage = u16::MAX;
        return;
    };

    state.hits = target.hits;

    let mut damages = Vec::new();
    let mut total = 0;
    for unit_id in state.units.clone() {
        let Some(attacker) = ctx.unit(unit_id) else {
            continue;
        };
        let damage = get_projected_damage(ctx.world, &attacker, &target);

        total += damage;
        damages.push(UnitDamage {
            unit: unit_id,
            damage,
        });
    }

    state.unit_damages = damages;
    state.required_damage = (i32::from(target.hits) - total).max(0) as u16;
}

/// `TaskKillUnit::FindVaildTypes` — типы юнитов, способные добить цель.
fn kill_unit_find_valid_types(
    world: &crate::game::GameState,
    team: TeamId,
    state: &mut super::KillUnitState,
) {
    state.weight_table.clear();
    state.weight_table.set_team(team.0);

    let Some(target_type) = state.target_type else {
        return;
    };

    // Таблица весов C++ строится `AiPlayer::GetExtendedWeightTable(target, flags)`;
    // здесь — те же типы, что проходят проверку `Access_IsValidAttackTargetType`.
    for unit_type in [
        UnitType::Tank,
        UnitType::AssaultGun,
        UnitType::RocketLauncher,
        UnitType::MissileCrawler,
        UnitType::MobileAntiAir,
        UnitType::Infiltrator,
        UnitType::Infantry,
        UnitType::Gunboat,
        UnitType::MissileCruiser,
        UnitType::Submarine,
        UnitType::Fighter,
        UnitType::Bomber,
    ] {
        if crate::access::is_valid_attack_target_type(unit_type, target_type) {
            state.weight_table.add(unit_type, 1);
        }
    }

    let _ = world;
}

// ---------------------------------------------------------------------------
// TaskKillUnit
// ---------------------------------------------------------------------------

fn kill_unit_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::KillUnit(state) = kind else {
        return;
    };

    let state = state.as_mut();

    // Цель могла быть уничтожена.
    let target_alive = state
        .spotted_unit
        .and_then(|index| spotted_target(ctx, header.team, index))
        .map(|(unit_id, _, _)| ctx.unit(unit_id).map(|info| info.hits > 0).unwrap_or(false))
        .unwrap_or(false);

    if !target_alive {
        ctx.remove_self(header.id);
        return;
    }

    kill_unit_update_demand(ctx, header.team, state);
    kill_unit_find_valid_types(ctx.world, header.team, state);

    if !state.seek_target {
        return;
    }

    if !is_attack_share_left(ctx, header.team, state) {
        return;
    }

    // `GetNewUnits` — рекрутинг в порядке пяти списков (только мобильные).
    let mut recruited = Vec::new();

    for kind_list in [UnitListKind::MobileLandSea, UnitListKind::MobileAir] {
        for unit_id in ctx.list(kind_list) {
            if !is_attack_share_left(ctx, header.team, state) {
                break;
            }

            if state.units.contains(&unit_id) {
                continue;
            }

            if !is_unit_suitable(ctx, header.team, state, unit_id) {
                continue;
            }

            let can_recruit = crate::ai::task::task_is_recruitable(
                ctx.tm,
                ctx.world,
                unit_id,
                header.team,
                ctx.priority_of(header.id),
            );

            if can_recruit {
                recruited.push(unit_id);
            }
        }
    }

    for unit_id in recruited {
        state.units.push(unit_id);
        state.recruit_turn = ctx.world.turn_counter;
        ctx.attach(header.id, unit_id);
        ctx.remind_move_finished(unit_id, true);
    }

    kill_unit_update_demand(ctx, header.team, state);
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

    // `AiAttack_ProcessAttack` — приказ атаки (в т.ч. `ORDER_MOVE_TO_ATTACK`).
    let issued = attack::process_attack(ctx.tm, ctx.world, unit, target_id);

    if issued {
        return true;
    }

    // Не достаёт: подойти к цели.
    let target_position = state.target_position;
    let dx = i32::from(attacker.grid.x) - i32::from(target_position.x);
    let dy = i32::from(attacker.grid.y) - i32::from(target_position.y);
    let range = attacker
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_RANGE);

    if dx * dx + dy * dy > range * range {
        let priority = ctx.priority_of(header.id);
        let move_kind = TaskKind::move_to(
            unit,
            target_position,
            (range * range) as u16,
            CautionLevel::AvoidNextTurnsFire,
        );
        ctx.append_task(attacker.team, Some(header.id), priority, move_kind);
    }

    true
}

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

    if !is_unit_suitable(ctx, header.team, state, unit) {
        return;
    }

    state.units.push(unit);
    ctx.attach(header.id, unit);
    ctx.remind_move_finished(unit, true);

    kill_unit_update_demand(ctx, header.team, state);
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
    state.unit_damages.retain(|damage| damage.unit != unit);

    if state.units.is_empty() {
        ctx.remove_self(header.id);
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
        ctx.remove_self(header.id);
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
        kill_unit_update_demand(ctx, header.team, state);
    }

    true
}

// ---------------------------------------------------------------------------
// TaskAttack
// ---------------------------------------------------------------------------

/// `TaskAttack::ChooseFirstTarget` — первый замеченный юнит целевой команды.
fn attack_choose_first_target(
    ctx: &mut TaskCtx<'_>,
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

/// `TaskAttack::DetermineLeader` — самый ценный юнит группы.
fn attack_determine_leader(ctx: &TaskCtx<'_>, state: &super::AttackState) -> Option<UnitId> {
    let mut leader: Option<(i32, UnitId)> = None;

    let mut units = state.primary_targets.clone();
    for task_id in units.drain(..) {
        if let Some(task) = ctx.tm.task(task_id) {
            if let TaskKind::KillUnit(kill) = &task.kind {
                for unit_id in &kill.units {
                    let Some(info) = ctx.unit(*unit_id) else {
                        continue;
                    };
                    let value = info
                        .base_values
                        .get_attribute(crate::unit_values::ATTRIB_HITS)
                        + info
                            .base_values
                            .get_attribute(crate::unit_values::ATTRIB_ATTACK);

                    if leader.map(|(best, _)| value > best).unwrap_or(true) {
                        leader = Some((value, *unit_id));
                    }
                }
            }
        }
    }

    leader.map(|(_, unit)| unit)
}

fn attack_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::Attack(state) = kind else {
        return;
    };

    let state = state.as_mut();

    if state.op_state == attack_state::INIT {
        state.op_state = attack_state::WAIT;
    }

    // Есть ли живая цель выбранной команды.
    let target_team = state.target_team.unwrap_or(header.team);

    let has_target = {
        let player = &ctx.world.ai.players[header.team.0 as usize];

        player.spotted_units.iter().any(|spotted| {
            spotted
                .unit
                .and_then(|unit_id| ctx.unit(unit_id))
                .map(|info| info.team == target_team && info.hits > 0)
                .unwrap_or(false)
        })
    };

    if !has_target {
        // Все цели уничтожены.
        for task in state.primary_targets.clone() {
            if ctx.tm.has_task(task) {
                ctx.tm.task_remove_self(ctx.world, task);
            }
        }
        state.primary_targets.clear();
        state.secondary_targets.clear();
        ctx.remove_self(header.id);
        return;
    }

    if state.kill_unit_task.is_none() {
        let Some(spotted_index) = attack_choose_first_target(ctx, header.team, target_team) else {
            return;
        };

        let (unit_id, position, unit_type) = match spotted_target(ctx, header.team, spotted_index) {
            Some(target) => target,
            None => return,
        };

        let mut kill = super::KillUnitState::default();
        kill.spotted_unit = Some(spotted_index);
        kill.target_position = position;
        kill.target_type = Some(unit_type);
        kill.required_damage = u16::MAX;
        kill.seek_target = true;
        kill.recruit_turn = -1;
        let _ = unit_id;

        let priority = attack_priority(ctx.world, header.team);
        let task_id = ctx.append_task(
            header.team,
            Some(header.id),
            priority,
            TaskKind::KillUnit(Box::new(kill)),
        );

        state.kill_unit_task = Some(task_id);
        state.primary_targets.push(task_id);
    } else if let Some(task_id) = state.kill_unit_task {
        if !ctx.tm.has_task(task_id) {
            state.kill_unit_task = None;
            return;
        }
    }

    state.leader = attack_determine_leader(ctx, state);
    state.defenders_fire = 0;
}

/// `TaskAttack::GetPriority` — приоритет родителя или атака по умолчанию.
fn attack_priority(world: &crate::game::GameState, team: TeamId) -> u16 {
    let _ = world;
    let _ = team;
    TASK_PRIORITY_ATTACK_DEFAULT
}

fn attack_add_unit(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader, unit: UnitId) {
    let TaskKind::Attack(state) = kind else {
        return;
    };

    let Some(kill_task) = state.kill_unit_task else {
        return;
    };

    ctx.tm.task_add_unit(ctx.world, kill_task, unit);
    let _ = header;
}

/// `TaskAttack::IsUnitUsable`.
pub(crate) fn attack_is_unit_usable(
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

    if info.flags
        & (crate::enums::unit_flags::MOBILE_AIR_UNIT
            | crate::enums::unit_flags::MOBILE_SEA_UNIT
            | crate::enums::unit_flags::MOBILE_LAND_UNIT)
        == 0
    {
        return false;
    }

    info.ammo > 0 && info.orders != UnitOrder::Disable
}

/// `TaskAttack`/`TaskKillUnit`/`TaskFrontalAttack`/`TaskRetreat` —
/// `IsUnitTransferable` (`task.cpp:571`) с учётом родителя.
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

/// `TaskKillUnit::IsUnitUsable`.
pub(crate) fn kill_unit_is_unit_usable(
    world: &crate::game::GameState,
    header: TaskHeader,
    state: &super::KillUnitState,
    unit: UnitId,
) -> bool {
    let Some(info) = world.unit(unit) else {
        return false;
    };

    let _ = header;
    let Some(target_type) = state.target_type else {
        return false;
    };

    info.ammo > 0
        && info.hits > 0
        && crate::access::is_valid_attack_target_type(info.unit_type, target_type)
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
        && info.flags & crate::enums::unit_flags::MOBILE_LAND_UNIT != 0
}

/// `TaskSupportAttack::IsUnitUsable` (`unit_flags` — класс поддерживающих).
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

    if ctx.ready_to_take_orders(unit_id) && info.delayed_reaction == 0 {
        if attack::evaluate_attack(ctx.tm, ctx.world, unit_id, true) {
            ctx.remove_self(header.id);
            return;
        }
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

    // Найти сопровождающего нужного типа.
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

    // `ORDER_MOVE_TO_UNIT` к цели.
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

/// `TaskRetreat::Search` — безопасная клетка в радиусе.
///
/// C++ перебирает клетки `ZoneWalker`-подобным циклом с `steps_in_direction`
/// и `direction`; здесь — тот же обход `ZoneWalker` от текущей позиции,
/// критерий — карта урона ИИ (`GetDamagePotentialMap`) и проходимость.
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
// Диспетчеризация
// ---------------------------------------------------------------------------

/// `Init` для combat-классов.
pub fn init(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::Attack(_) => true,
        TaskKind::KillUnit(_) => true,
        TaskKind::FrontalAttack { .. } => true,
        TaskKind::WaitToAttack { .. } => true,
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

/// `BeginTurn` для combat-классов.
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
            attack_reserve_begin_turn(kind, ctx, header);
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

/// `EndTurn` для combat-классов.
pub fn end_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
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
            attack_reserve_begin_turn(kind, ctx, header);
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

/// `TaskSupportAttack::BeginTurn` — собрать поддерживающие юниты.
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

/// `TaskAttackReserve::BeginTurn` — резерв следует за атакующими к сайту.
fn attack_reserve_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::AttackReserve { units, site, .. } = kind else {
        return;
    };

    units.retain(|unit| ctx.unit(*unit).map(|info| info.hits > 0).unwrap_or(false));

    if units.is_empty() {
        ctx.remove_self(header.id);
        return;
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

        if dx * dx + dy * dy > 4 {
            let priority = ctx
                .priority_of(header.id)
                .min(TASK_PRIORITY_ATTACK_RESERVE_POOL);
            let move_kind = TaskKind::move_to(unit_id, *site, 4, CautionLevel::AvoidNextTurnsFire);
            ctx.append_task(info.team, Some(header.id), priority, move_kind);
        }
    }
}

/// `TaskDefenseReserve::BeginTurn` — резерв обороны у сайта.
fn defense_reserve_begin_turn(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) {
    let TaskKind::DefenseReserve { units, site } = kind else {
        return;
    };

    units.retain(|unit| ctx.unit(*unit).map(|info| info.hits > 0).unwrap_or(false));

    if units.is_empty() {
        // Рекрутинг: первый подходящий юнит команды.
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

/// `Execute(unit)` для combat-классов.
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
        TaskKind::WaitToAttack { .. } => {
            wait_to_attack_run(kind, ctx, header);
            true
        }
        TaskKind::AttackReserve { .. } => {
            attack_reserve_begin_turn(kind, ctx, header);
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

/// `AddUnit(unit)` для combat-классов.
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
        TaskKind::AttackReserve { units, .. } | TaskKind::DefenseReserve { units, .. } => {
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

/// `RemoveUnit(unit)` для combat-классов.
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
        TaskKind::SupportAttack { units, .. }
        | TaskKind::AttackReserve { units, .. }
        | TaskKind::DefenseReserve { units, .. } => {
            units.retain(|id| *id != unit);

            if units.is_empty() {
                ctx.remove_self(header.id);
            }
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

/// `RemoveSelf()` для combat-классов.
pub fn remove_self(kind: &mut TaskKind, ctx: &mut TaskCtx<'_>, header: TaskHeader) -> bool {
    match kind {
        TaskKind::KillUnit(_)
        | TaskKind::Attack(_)
        | TaskKind::FrontalAttack { .. }
        | TaskKind::WaitToAttack { .. }
        | TaskKind::SupportAttack { .. }
        | TaskKind::AttackReserve { .. }
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

/// `ChildComplete(task)` для combat-классов.
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

            true
        }
        TaskKind::KillUnit(_)
        | TaskKind::FrontalAttack { .. }
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

/// Уведомление о завершении дочернего `TaskMove`.
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
            let attacker = match kind {
                TaskKind::WaitToAttack { attacker } => *attacker,
                _ => None,
            };

            if let Some(unit_id) = attacker {
                let ready = ctx
                    .unit(unit_id)
                    .map(|info| {
                        info.hits > 0 && info.shots > 0 && !ctx.ready_to_take_orders(unit_id)
                    })
                    .unwrap_or(false);

                if !ready && attack::evaluate_attack(ctx.tm, ctx.world, unit_id, true) {
                    ctx.remove_self(header.id);
                    return true;
                }
            }

            false
        }
        TaskKind::KillUnit(_) => false,
        _ => false,
    }
}

/// `ExchangeOperator` — `TaskEscort` (обмен сопровождающего).
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

/// `EventUnitDestroyed(unit)` для combat-классов.
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
        TaskKind::SupportAttack { units, .. }
        | TaskKind::AttackReserve { units, .. }
        | TaskKind::DefenseReserve { units, .. } => {
            units.retain(|id| *id != unit);
            if units.is_empty() {
                ctx.remove_self(header.id);
            }
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

/// `EventEnemyUnitSpotted(unit)` для combat-классов.
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

/// `EventCargoTransfer` — не переопределён.
pub fn event_cargo_transfer(
    _kind: &mut TaskKind,
    _ctx: &mut TaskCtx<'_>,
    _header: TaskHeader,
    _unit: UnitId,
) -> bool {
    false
}

/// `EventUnitLoaded` — `TaskEscort`/`TaskSupportAttack` (структурно).
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

/// `EventUnitUnloaded` — структурно.
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

/// `TaskAttack::GetCautionLevel` — в C++ зависит от роли юнита в группе.
pub fn attack_caution_level(
    world: &crate::game::GameState,
    team: TeamId,
    unit: &UnitInfo,
) -> CautionLevel {
    let _ = world;
    let _ = team;

    if unit.shots > 0 {
        CautionLevel::AvoidReactionFire
    } else {
        CautionLevel::AvoidNextTurnsFire
    }
}
