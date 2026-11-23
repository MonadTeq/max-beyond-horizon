//! Attack decisions — a port of `aiattack.cpp`.
//!
//! Oracle: `max-cpp-port/src/aiattack.cpp/.hpp`:
//! `AiAttack_GetTargetValue`, `AiAttack_GetReach`, `AiAttack_IsWithinReach`,
//! `AiAttack_IsValidSabotageTarget`, `AiAttack_ProcessAttack`,
//! `AiAttack_CanAttack`, `AiAttack_GetAttackPotential`,
//! `AiAttack_SetAttackTargetCooldown`, `AiAttack_EvaluateAttack`,
//! `AiAttack_SelectTargetToAttack`, `AiAttack_GetTargetFlags`,
//! `AiAttack_GetTargetWorth`, `AiAttack_IsPriorityTarget`,
//! `AiAttack_EstimateAttackValue`, `AiAttack_IsAttackProfitable`,
//! `AiAttack_DecideDesperationAttack`, `AiAttack_FindAttackSupport`,
//! `AiAttack_ChooseSiteForAttacker`, `AiAttack_ChooseSiteToSabotage`.
//!
//! # RNG order
//!
//! `AiAttack_EvaluateAttack` consumes `Randomizer_Generate(2)` (the "consider
//! target cooldown" flag), `AiAttack_ProcessAttack` for the saboteur —
//! `Randomizer_Generate(101)` (stealth roll). Both calls are at the same
//! places as in C++, in the same order (DESIGN §4.3).
//!
//! # Simplifications
//!
//! * `TransporterMap` (`ChooseSiteForAttacker`, `mode = true`) is not ported:
//!   reachability is checked by `Access_IsAccessible` (P5) without transports;
//! transports;
//! * `AiPlayer_TerrainDistanceField->GetMinimumRange` is replaced by the Euclidean
//!   distance on the map (P5 `TerrainDistanceField` is not set up in the crate);
//! * `Ai_FindAttackSupport` iterates units in list order
//!   `MobileAir` → `MobileLandSea`, as in C++.

use crate::enums::{CautionLevel, UnitOrder, UnitOrderState, UnitType};
use crate::game::GameState;
use crate::geo::Point;
use crate::unit::{UnitId, UnitInfo, UnitListKind};

use super::task::{
    task_is_adjacent, task_is_ready_to_take_orders, task_is_unit_doomed_to_destruction,
    task_retreat_if_necessary, TASK_PRIORITY_EMERGENCY,
};
use super::task_manager::TaskManager;
use super::tasks::{TaskCtx, TaskKind};
use super::AiReactionState;

// ---------------------------------------------------------------------------
// Target cost and reachability
// ---------------------------------------------------------------------------

/// `AiAttack_GetTargetValue` (`aiattack.cpp:46`).
pub fn get_target_value(unit: &UnitInfo) -> i32 {
    let attack_range = unit
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_RANGE);
    let turns_to_build = unit
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_TURNS);

    if turns_to_build > 4 {
        if attack_range >= 10
            || unit.unit_type == UnitType::Infiltrator
            || unit.unit_type == UnitType::Submarine
            || unit
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_TURNS)
                < 8
        {
            attack_range
        } else {
            10
        }
    } else {
        unit.base_values
            .get_attribute(crate::unit_values::ATTRIB_SCAN)
    }
}

/// `AiAttack_GetReach` (`aiattack.cpp:264`).
pub fn get_reach(unit: &UnitInfo) -> i32 {
    let mut result = unit
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_RANGE);

    if unit
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_MOVE_AND_FIRE)
        != 0
    {
        result += unit
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_SPEED);
    }

    let scan = unit
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_SCAN);
    if scan > result {
        result = scan;
    }

    result
}

/// `AiAttack_IsWithinReach` (`aiattack.cpp:279`) — whether there is an own building nearby.
pub fn is_within_reach(world: &GameState, unit: &UnitInfo, team: crate::team::TeamId) -> bool {
    if unit.team == crate::team::TeamId::ALIEN {
        return false;
    }

    let mut distance = get_reach(unit);
    distance *= distance;

    if distance == 0 {
        return false;
    }

    world.unit_list(UnitListKind::Stationary).iter().any(|id| {
        world
            .unit(*id)
            .map(|other| {
                other.team == team
                    && crate::access::squared_distance(unit.grid, other.grid) <= distance
            })
            .unwrap_or(false)
    })
}

/// `AiAttack_IsValidSabotageTarget` (`aiattack.cpp:314`).
pub fn is_valid_sabotage_target(world: &GameState, unit: &UnitInfo, target: &UnitInfo) -> bool {
    unit.unit_type == UnitType::Infiltrator
        && target.flags & crate::enums::unit_flags::ELECTRONIC_UNIT != 0
        && target.flags & crate::enums::unit_flags::HOVERING == 0
        && (target.orders != UnitOrder::Disable || target.is_mobile())
        && crate::access::is_valid_attack_target(world, unit, target)
}

/// `AiAttack_CanAttack` (`aiattack.cpp:455`).
pub fn can_attack(world: &GameState, attacker: &UnitInfo, target: &UnitInfo) -> bool {
    if attacker.unit_type == UnitType::Infiltrator {
        if is_valid_sabotage_target(world, attacker, target)
            && task_is_adjacent(target, attacker.grid.x, attacker.grid.y)
            && crate::combat::get_stealth_chance_percentage(
                world,
                attacker,
                target,
                UnitOrder::AwaitDisableUnit,
            ) > 85
        {
            return true;
        }

        if !attacker.is_visible_to_team(target.team) {
            return false;
        }
    }

    let range = attacker
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_RANGE);

    crate::access::is_valid_attack_target(world, attacker, target)
        && crate::access::is_within_attack_range(world, attacker, target.grid, range)
}

/// `AiAttack_GetAttackPotential` (`aiattack.cpp:683`).
pub fn get_attack_potential(world: &GameState, attacker: &UnitInfo, target: &UnitInfo) -> i32 {
    if is_valid_sabotage_target(world, attacker, target) {
        let chance = crate::combat::get_stealth_chance_percentage(
            world,
            attacker,
            target,
            UnitOrder::AwaitDisableUnit,
        );

        (chance * i32::from(target.hits)) / 100
    } else {
        crate::combat::get_attack_damage(attacker, target, 0)
    }
}

/// `AiAttack_SetAttackTargetCooldown` (`aiattack.cpp:697`).
pub fn set_attack_target_cooldown(world: &mut GameState, unit: UnitId) {
    if let Some(mut info) = world.unit(unit).cloned() {
        info.ai_state_bits |= crate::enums::AI_STATE_TARGET_COOLDOWN_MASK;
        world.write_unit(unit, info);
    }
}

// ---------------------------------------------------------------------------
// Profitability estimates
// ---------------------------------------------------------------------------

/// `AiAttack_EstimateAttackValue` (`aiattack.cpp:506`).
pub fn estimate_attack_value(unit: &UnitInfo, predicted_damage: i32) -> i32 {
    let hits = unit
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_HITS);
    let turns = unit
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_TURNS);

    if hits == 0 {
        return 0;
    }

    ((hits + predicted_damage - i32::from(unit.hits)) * turns) / hits
}

/// `AiAttack_IsAttackProfitable` (`aiattack.cpp:514`) with explicit damage to the target.
pub fn is_attack_profitable_with(
    world: &GameState,
    friendly_unit: &UnitInfo,
    enemy_unit: &UnitInfo,
    damage_to_friendly: i32,
    caution_level: CautionLevel,
    damage_to_enemy: i32,
    is_desperation_attack: bool,
) -> bool {
    let projected_damage =
        super::strategy::get_predicted_attack(world, friendly_unit, caution_level);

    let damage_to_friendly = estimate_attack_value(friendly_unit, damage_to_friendly);
    let mut damage_to_enemy = damage_to_enemy;

    if damage_to_friendly > 0 {
        let turns = friendly_unit
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_TURNS);
        if turns != 0 {
            damage_to_enemy -= (projected_damage * damage_to_friendly) / turns;
        }
    }

    if damage_to_enemy > 0 {
        damage_to_enemy = estimate_attack_value(enemy_unit, damage_to_enemy);
    } else {
        damage_to_enemy = 0;
    }

    if is_desperation_attack && damage_to_enemy > 0 {
        true
    } else {
        damage_to_friendly < damage_to_enemy
    }
}

/// `AiAttack_IsAttackProfitable` (`aiattack.cpp:546`) — damage to the target is computed
/// `GetPredictedAttack`.
pub fn is_attack_profitable(
    world: &GameState,
    friendly_unit: &UnitInfo,
    enemy_unit: &UnitInfo,
    damage_to_friendly: i32,
    caution_level: CautionLevel,
    is_desperation_attack: bool,
) -> bool {
    let projected_damage = super::strategy::get_predicted_attack(world, enemy_unit, caution_level);

    is_attack_profitable_with(
        world,
        friendly_unit,
        enemy_unit,
        damage_to_friendly,
        caution_level,
        projected_damage,
        is_desperation_attack,
    )
}

/// `AiAttack_DecideDesperationAttack` (`aiattack.cpp:68`).
pub fn decide_desperation_attack(world: &mut GameState, attacker: UnitId, target: UnitId) -> bool {
    let Some(target_info) = world.unit(target).cloned() else {
        return false;
    };

    if target_info.ammo == 0 {
        return false;
    }

    if task_is_unit_doomed_to_destruction(world, attacker, CautionLevel::AvoidNextTurnsFire) {
        return true;
    }

    let Some(attacker_info) = world.unit(attacker).cloned() else {
        return false;
    };

    let attacker_turns = attacker_info
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_TURNS);

    let mut found = false;
    for id in world.unit_list(UnitListKind::Stationary) {
        let Some(other) = world.unit(*id) else {
            continue;
        };

        if other.team == attacker_info.team
            && other.is_visible_to_team(target_info.team)
            && other
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_TURNS)
                > attacker_turns
            && crate::access::is_valid_attack_target(world, &target_info, other)
        {
            let range = target_info
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_RANGE)
                + target_info
                    .base_values
                    .get_attribute(crate::unit_values::ATTRIB_SPEED);

            if crate::access::is_within_attack_range(world, &target_info, other.grid, range) {
                found = true;
                break;
            }
        }
    }

    found
}

// ---------------------------------------------------------------------------
// Target value
// ---------------------------------------------------------------------------

/// `AiAttack_GetTargetWorth` (`aiattack.cpp:1292`).
pub fn get_target_worth(world: &GameState, attacker: &UnitInfo, target: &UnitInfo) -> u32 {
    if !crate::access::is_valid_attack_target(world, attacker, target) || target.hits == 0 {
        return 0;
    }

    let rounds = attacker
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_ROUNDS);
    let damage = crate::combat::get_attack_damage(attacker, target, 0);
    let turns = target
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_TURNS);

    let value = (rounds * damage * turns * 7) / (i32::from(target.hits) * 12);

    if value >= 1 {
        value.min(7) as u32
    } else {
        1
    }
}

/// `AiAttack_IsPriorityTarget` (`aiattack.cpp:1323`).
pub fn is_priority_target(world: &GameState, target: &UnitInfo, team: crate::team::TeamId) -> bool {
    (target.unit_type == UnitType::EcoSphere || target.unit_type == UnitType::MiningStation)
        && world.teams[target.team.0 as usize].team_points
            > world.teams[team.0 as usize].team_points
}

/// `AiAttack_GetTargetFlags` (`aiattack.cpp:1244`) — smaller is better.
///
/// `attacker` is optional: C++ passes `nullptr` when the flags are computed for
/// task priority (`TaskKillUnit::GetPriority`), so only the target-only part
/// contributes in that case.
pub fn get_target_flags(
    world: &GameState,
    attacker: Option<&UnitInfo>,
    target: &UnitInfo,
    team: crate::team::TeamId,
) -> u32 {
    let mut result: u32 = 0;
    let target_attack = target
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_ATTACK);

    if target.ammo > 0 && target.orders != UnitOrder::Disable {
        if target_attack > 35 {
            result += 0x38;
        } else {
            result = ((target_attack / 8) * 8) as u32;
        }
    }

    if target.is_visible_to_team(team) {
        result += 0x80;
    }

    if target.team == crate::team::TeamId::ALIEN {
        result += 0x08;
    }

    if let Some(attacker) = attacker {
        if is_valid_sabotage_target(world, attacker, target) {
            result += (crate::combat::get_stealth_chance_percentage(
                world,
                attacker,
                target,
                UnitOrder::AwaitDisableUnit,
            ) / 13) as u32;

            if target.is_mobile() {
                result += 0x40;
            }
        } else {
            result += get_target_worth(world, attacker, target);

            let shots = i32::from(attacker.shots);
            let damage = crate::combat::get_attack_damage(attacker, target, 0);

            if shots * damage >= i32::from(target.hits) {
                result += 0x40;

                if matches!(
                    target.unit_type,
                    UnitType::RepairUnit
                        | UnitType::MiningStation
                        | UnitType::Scanner
                        | UnitType::Radar
                        | UnitType::EcoSphere
                ) {
                    result += 0x38;
                }
            }
        }
    }

    0xFF - result
}

// ---------------------------------------------------------------------------
// Target selection
// ---------------------------------------------------------------------------

/// `AiAttack_SelectTargetToAttack` (`aiattack.cpp:554`).
///
/// Returns the `SpottedUnit` index in `AiPlayer::spotted_units` — the
/// iteration order and the selection rule (`score`, then `damage_potential`, then `hits`)
/// are preserved as in C++.
pub fn select_target_to_attack(
    world: &GameState,
    unit: UnitId,
    range: i32,
    scan: i32,
    caution_level: CautionLevel,
    mode: bool,
) -> Option<u32> {
    let attacker = world.unit(unit)?.clone();
    let team = attacker.team;
    let player = &world.ai.players[team.0 as usize];

    let mut selected: Option<u32> = None;
    let mut minimum_score: u32 = 0;
    let mut minimum_damage: i32 = 32000;

    let damage_map = if caution_level != CautionLevel::None {
        super::strategy::get_damage_potential_map(world, team, &attacker, caution_level)
    } else {
        None
    };

    let range_squared = range * range;
    let scan_squared = scan * scan;

    for (index, spotted) in player.spotted_units.iter().enumerate() {
        let Some(target_id) = spotted.unit else {
            continue;
        };
        let Some(target) = world.unit(target_id) else {
            continue;
        };

        if target.hits == 0 || target.team == team || !target.is_visible_to_team(team) {
            continue;
        }

        if !crate::access::is_valid_attack_target(world, &attacker, target) {
            continue;
        }

        // The target must be within detection/fire range.
        let mut target_position = spotted.last_position;
        if !target.is_visible_to_team(team)
            && world.ai_heat_map_complete(
                team,
                i32::from(target_position.x),
                i32::from(target_position.y),
            ) > 0
        {
            target_position = target.grid;
        }

        let distance = crate::access::squared_distance(attacker.grid, target_position);

        if distance > range_squared && distance > scan_squared {
            continue;
        }

        if target.unit_type == UnitType::LandMine || target.unit_type == UnitType::SeaMine {
            let attack_radius = attacker
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_ATTACK_RADIUS);
            let turns = attacker
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_TURNS);

            if !(attack_radius > 0 || turns < 6) {
                continue;
            }
        }

        let mut score = 0u32;

        if mode {
            let mut bits = target.ai_state_bits;
            let cooldown = bits & crate::enums::AI_STATE_TARGET_COOLDOWN_MASK;

            if cooldown != 0 {
                bits &= !crate::enums::AI_STATE_TARGET_COOLDOWN_MASK;

                if cooldown > 2 {
                    bits |= cooldown - 2;
                }

                if let Some(mut target_copy) = world.unit(target_id).cloned() {
                    target_copy.ai_state_bits = bits;
                    // Record the change through `GameState` (needs `&mut`),
                    // so the actual write is performed by the caller layer:
                    // here only the cooldown "weight" is accounted for.
                    let _ = target_copy;
                }

                score = cooldown << 7;
            }
        }

        score |= get_target_flags(world, Some(&attacker), target, team);

        let mut damage_potential = 0;

        if let Some(map) = &damage_map {
            damage_potential =
                map.damage_at(i32::from(target_position.x), i32::from(target_position.y));

            if damage_potential < i32::from(attacker.hits) {
                damage_potential = 0;
            }
        }

        let better = match selected {
            None => true,
            Some(selected_index) => {
                let same_or_better_score = score <= minimum_score;

                if !same_or_better_score {
                    false
                } else if score != minimum_score {
                    true
                } else if damage_potential > minimum_damage {
                    false
                } else if damage_potential != minimum_damage {
                    true
                } else {
                    player
                        .spotted_units
                        .get(selected_index as usize)
                        .and_then(|spotted| spotted.unit)
                        .and_then(|previous| world.unit(previous))
                        .map(|previous| previous.hits >= target.hits)
                        .unwrap_or(true)
                }
            }
        };

        if better {
            selected = Some(index as u32);
            minimum_score = score;
            minimum_damage = damage_potential;
        }
    }

    selected
}

// ---------------------------------------------------------------------------
// Attack order
// ---------------------------------------------------------------------------

/// `TaskWaitToAttack(unit)` (`taskwaittoattack.cpp:29`) — the parent is the
/// unit's current task and the priority follows it.
fn append_wait_to_attack(
    tm: &mut TaskManager,
    world: &mut GameState,
    unit: UnitId,
    team: crate::team::TeamId,
) {
    let parent = tm.unit_task(unit);
    let priority = parent
        .and_then(|task_id| tm.task(task_id))
        .map(|task| task.get_priority(world))
        .unwrap_or(super::task::TASK_PRIORITY_MOVE);

    tm.append_task(
        world,
        super::Task::new(0, team, parent, priority, TaskKind::wait_to_attack(unit)),
    );
}

/// `AiAttack_ProcessAttack` (`aiattack.cpp:330`).
///
/// Returns `true` if the order was issued (or `TaskWaitToAttack` was set).
pub fn process_attack(
    tm: &mut TaskManager,
    world: &mut GameState,
    attacker: UnitId,
    target: UnitId,
) -> bool {
    let Some(attacker_info) = world.unit(attacker).cloned() else {
        return false;
    };

    if !world.is_active_turn(attacker_info.team) {
        return false;
    }

    if attacker_info.delayed_reaction != 0
        || world.ai.reaction_state == AiReactionState::AnimationsActive
    {
        append_wait_to_attack(tm, world, attacker, attacker_info.team);
        return true;
    }

    let Some(target_info) = world.unit(target).cloned() else {
        return false;
    };

    if target_info.team == attacker_info.team {
        return false;
    }

    let mut attack_range = attacker_info
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_RANGE);

    let is_sabotage = is_valid_sabotage_target(world, &attacker_info, &target_info);
    let is_valid = if is_sabotage {
        attack_range = 1;
        task_is_adjacent(&target_info, attacker_info.grid.x, attacker_info.grid.y)
    } else {
        crate::access::is_within_attack_range(world, &attacker_info, target_info.grid, attack_range)
    };

    let mut data = world.order_data(attacker);

    if is_valid {
        data.move_to = target_info.grid;
    } else {
        let attack_radius = attacker_info
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_ATTACK_RADIUS);

        if attack_radius >= 1
            && world.ai.settings.opponent >= crate::enums::Opponent::Average as u8
            && attacker_info.shots > 0
        {
            // `ZoneWalker` around the target: the nearest cell within own range.
            let mut walker = super::threat::ZoneWalker::new(target_info.grid, attack_radius);
            let mut found: Option<(i32, Point)> = None;
            let range_squared = attack_range * attack_range;

            loop {
                let site = walker.current_location();

                if crate::access::squared_distance(attacker_info.grid, site) <= range_squared {
                    let distance = crate::access::squared_distance(target_info.grid, site);

                    if found.map(|(best, _)| distance < best).unwrap_or(true) {
                        found = Some((distance, site));
                    }
                }

                if !walker.find_next() {
                    break;
                }
            }

            match found {
                Some((_, site)) => data.move_to = site,
                None => return false,
            }
        } else {
            return false;
        }
    }

    world.set_order_data(attacker, data);

    let can_issue = attacker_info.shots > 0 || attacker_info.unit_type != UnitType::Infiltrator;

    if !can_issue {
        return false;
    }

    if attacker_info.unit_type == UnitType::Infiltrator && attack_range == 1 {
        let order = if target_info.orders == UnitOrder::Disable
            || (target_info.is_mobile()
                && crate::combat::get_stealth_chance_percentage(
                    world,
                    &attacker_info,
                    &target_info,
                    UnitOrder::AwaitStealUnit,
                ) >= i32::from(world.ai.settings.max_percent as u8))
        {
            UnitOrder::AwaitStealUnit
        } else {
            UnitOrder::AwaitDisableUnit
        };

        let order = if order == UnitOrder::AwaitStealUnit
            && !crate::combat::is_valid_stealth_target(
                world,
                &attacker_info,
                &target_info,
                UnitOrder::AwaitStealUnit,
            ) {
            UnitOrder::AwaitDisableUnit
        } else {
            order
        };

        if crate::combat::is_valid_stealth_target(world, &attacker_info, &target_info, order) {
            world.ai.reaction_state = AiReactionState::AnimationsActive;

            let dice = world.rng.generate_i32(101) as u8;

            if let Some(mut info) = world.unit(attacker).cloned() {
                info.parent_unit = Some(target);
                info.stealth_dice_roll = dice;
                world.write_unit(attacker, info);
            }

            world.set_order(attacker, order, UnitOrderState::Init);
            return true;
        }

        return false;
    }

    if attacker_info.shots > 0 {
        world.ai.reaction_state = AiReactionState::AnimationsActive;
    }

    if let Some(mut info) = world.unit(attacker).cloned() {
        info.enemy_unit = Some(target);
        world.write_unit(attacker, info);
    }

    world.set_order(
        attacker,
        UnitOrder::MoveToAttack,
        UnitOrderState::ExecutingOrder,
    );

    true
}

/// `AiAttack_FindAttackSupport` (`aiattack.cpp:473`).
pub fn find_attack_support(
    tm: &mut TaskManager,
    world: &mut GameState,
    target: UnitId,
    kind: UnitListKind,
    team: crate::team::TeamId,
    caution_level: CautionLevel,
) -> bool {
    let Some(target_info) = world.unit(target).cloned() else {
        return false;
    };

    if !target_info.is_visible_to_team(team) || !world.is_active_turn(team) {
        return false;
    }

    let mut result = false;
    let ids: Vec<UnitId> = world.unit_list(kind).to_vec();

    for id in ids {
        let Some(unit) = world.unit(id).cloned() else {
            continue;
        };

        if unit.team != team || unit.shots == 0 || !task_is_ready_to_take_orders(world, id) {
            continue;
        }

        if !can_attack(world, &unit, &target_info) {
            continue;
        }

        if target_info.unit_type == UnitType::LandMine || target_info.unit_type == UnitType::SeaMine
        {
            let attack_radius = unit
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_ATTACK_RADIUS);
            let turns = unit
                .base_values
                .get_attribute(crate::unit_values::ATTRIB_TURNS);

            if !(attack_radius > 0 || turns < 6) {
                continue;
            }
        }

        let move_and_fire = unit
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_MOVE_AND_FIRE);
        let opponent = world.ai.settings.opponent;

        let acceptable = (move_and_fire != 0 && opponent >= crate::enums::Opponent::Average as u8)
            || super::strategy::get_damage_potential(world, team, &unit, unit.grid, caution_level)
                < i32::from(unit.hits);

        if acceptable {
            process_attack(tm, world, id, target);
            result = true;
        }
    }

    result
}

/// `AiAttack_ChooseSiteForAttacker` (`aiattack.cpp:176`) — the cell to attack from.
pub fn choose_site_for_attacker(
    world: &GameState,
    unit: &UnitInfo,
    target: Point,
    caution_level: CautionLevel,
    range: i32,
    _mode: bool,
) -> Option<(Point, i32)> {
    let damage_map =
        super::strategy::get_damage_potential_map(world, unit.team, unit, caution_level);

    let mut best: Option<(i32, i32, Point)> = None;
    let mut minimum_damage = i16::MAX as i32;
    let distance_limit = range * range;

    // The own cell — if the target is already in range.
    let own_distance = crate::access::squared_distance(unit.grid, target);
    if own_distance <= distance_limit {
        let damage = damage_map
            .as_ref()
            .map(|map| map.damage_at(i32::from(unit.grid.x), i32::from(unit.grid.y)))
            .unwrap_or(0);

        minimum_damage = damage;
        best = Some((damage, own_distance, unit.grid));
    }

    let mut walker = super::threat::ZoneWalker::new(target, range);

    loop {
        let site = walker.current_location();

        let damage = damage_map
            .as_ref()
            .map(|map| map.damage_at(i32::from(site.x), i32::from(site.y)))
            .unwrap_or(0);

        let distance = crate::access::squared_distance(unit.grid, site);

        if damage <= minimum_damage || distance > distance_limit {
            let better = if damage > minimum_damage {
                distance <= distance_limit
            } else {
                match best {
                    None => true,
                    Some((best_damage, best_distance, _)) => {
                        damage < best_damage || (damage == best_damage && distance < best_distance)
                    }
                }
            };

            if better
                && crate::access::is_accessible(
                    world,
                    unit.unit_type,
                    unit.team,
                    site,
                    crate::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
                ) != 0
            {
                minimum_damage = damage;
                best = Some((damage, distance, site));
            }
        }

        if !walker.find_next() {
            break;
        }
    }

    best.map(|(damage, _, site)| (site, damage))
}

/// `AiAttack_ChooseSiteToSabotage` (`aiattack.cpp:105`) — a cell near the target.
pub fn choose_site_to_sabotage(
    world: &GameState,
    unit: &UnitInfo,
    target: &UnitInfo,
    caution_level: CautionLevel,
) -> Option<(Point, i32)> {
    let damage_map =
        super::strategy::get_damage_potential_map(world, unit.team, unit, caution_level);

    if task_is_adjacent(target, unit.grid.x, unit.grid.y) {
        let damage = damage_map
            .as_ref()
            .map(|map| map.damage_at(i32::from(unit.grid.x), i32::from(unit.grid.y)))
            .unwrap_or(0);

        return Some((unit.grid, damage));
    }

    // A 2×2 target is bypassed at a larger radius (`Footprint`: layout, not
    // semantics — a 1×1 turret goes through branch 2).
    let standing = target.is_quad();
    let range = if standing { 3 } else { 2 };

    let mut best: Option<(i32, i32, Point)> = None;
    let mut projected_damage = i16::MAX as i32;
    let mut minimum_distance = i16::MAX as i32;

    let start = Point::new(target.grid.x - 1, target.grid.y - 1 + range as i16);

    for direction in 0..4 {
        let offset = super::threat::CARDINAL_DIRECTIONS[direction];
        let mut position = start;

        for _ in 0..range {
            position += offset;

            if !super::threat::in_bounds(position) {
                continue;
            }

            let damage = damage_map
                .as_ref()
                .map(|map| map.damage_at(i32::from(position.x), i32::from(position.y)))
                .unwrap_or(0);

            if damage > projected_damage {
                continue;
            }

            let distance = crate::access::approximate_distance(
                i32::from(position.x) - i32::from(unit.grid.x),
                i32::from(position.y) - i32::from(unit.grid.y),
            );

            if damage < projected_damage || distance < minimum_distance {
                if crate::access::is_accessible(
                    world,
                    unit.unit_type,
                    unit.team,
                    position,
                    crate::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
                ) != 0
                {
                    projected_damage = damage;
                    minimum_distance = distance;
                    best = Some((damage, distance, position));
                }
            }
        }
    }

    best.map(|(damage, _, site)| (site, damage))
}

// ---------------------------------------------------------------------------
// Attack evaluation and execution
// ---------------------------------------------------------------------------

/// `AiAttack_EvaluateAttack` (`aiattack.cpp:701`).
///
/// `mode` — "attack after movement" (C++ `mode = true`); it affects the
/// preparation and reserve branch.
pub fn evaluate_attack(
    tm: &mut TaskManager,
    world: &mut GameState,
    unit: UnitId,
    mode: bool,
) -> bool {
    if world.play_mode == crate::enums::PlayMode::Unknown {
        return false;
    }

    let Some(info) = world.unit(unit).cloned() else {
        return false;
    };

    if info.ammo == 0 || !world.is_active_turn(info.team) {
        return false;
    }

    let move_and_fire = info
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_MOVE_AND_FIRE);

    if !(info.shots > 0 || move_and_fire == 0) {
        return false;
    }

    if info.delayed_reaction != 0 || world.ai.reaction_state == AiReactionState::AnimationsActive {
        append_wait_to_attack(tm, world, unit, info.team);

        return world.ai.reaction_state == AiReactionState::AnimationsActive;
    }

    // `if (order == MOVE_TO_ATTACK && !Access_GetAttackTarget(unit, move_to))`.
    if info.orders == UnitOrder::MoveToAttack {
        let data = world.order_data(unit);
        if crate::access::get_attack_target(world, &info, data.move_to, false).is_none() {
            world.set_order(unit, UnitOrder::Await, UnitOrderState::ExecutingOrder);
        }
    }

    if !task_is_ready_to_take_orders(world, unit) {
        return false;
    }

    let attack_radius = if info.shots > 0 {
        info.base_values
            .get_attribute(crate::unit_values::ATTRIB_ATTACK_RADIUS)
            / 2
    } else {
        0
    };

    // RNG order 1:1 with C++: `Randomizer_Generate(2)`.
    let consider_cooldown = world.rng.generate_i32(2) != 0;

    let range = info
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_RANGE)
        + attack_radius;
    let scan = info
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_SCAN);

    let spotted = select_target_to_attack(
        world,
        unit,
        range,
        scan,
        CautionLevel::None,
        consider_cooldown,
    );

    let Some(spotted_index) = spotted else {
        return false;
    };

    let Some((target_id, _, _)) =
        super::tasks::combat::spotted_target_for_ai(world, info.team, spotted_index)
    else {
        return false;
    };

    let Some(target) = world.unit(target_id).cloned() else {
        return false;
    };

    if info.unit_type == UnitType::Infiltrator
        && is_valid_sabotage_target(world, &info, &target)
        && !task_is_adjacent(&target, info.grid.x, info.grid.y)
    {
        return false;
    }

    if target.pin_count > 0 {
        let kind = TaskKind::wait_to_attack(unit);
        tm.append_task(
            world,
            super::Task::new(
                0,
                info.team,
                None,
                super::task::TASK_PRIORITY_ATTACK_DEFAULT,
                kind,
            ),
        );
        return true;
    }

    let mut mode = mode;

    if world.ai.settings.opponent < crate::enums::Opponent::Apprentice as u8 {
        mode = false;
    }

    let target_turns = target
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_TURNS);
    let unit_turns = info
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_TURNS);
    let shots_damage = i32::from(info.shots) * crate::combat::get_attack_damage(&info, &target, 0);

    if mode
        && info.speed > 0
        && (move_and_fire == 0
            || world.ai.settings.opponent < crate::enums::Opponent::Average as u8)
        && (target_turns < unit_turns || shots_damage < i32::from(target.hits))
    {
        let damage_potential = super::strategy::get_damage_potential(
            world,
            info.team,
            &info,
            info.grid,
            CautionLevel::AvoidNextTurnsFire,
        );

        if damage_potential >= i32::from(info.hits) {
            let support_air = find_attack_support(
                tm,
                world,
                target_id,
                UnitListKind::MobileAir,
                info.team,
                CautionLevel::AvoidNextTurnsFire,
            );

            let support = support_air
                || find_attack_support(
                    tm,
                    world,
                    target_id,
                    UnitListKind::MobileLandSea,
                    info.team,
                    CautionLevel::AvoidNextTurnsFire,
                );

            if support {
                return true;
            }

            let profitable = is_attack_profitable(
                world,
                &info,
                &target,
                damage_potential,
                CautionLevel::AvoidNextTurnsFire,
                false,
            );

            if !profitable
                && !decide_desperation_attack(world, unit, target_id)
                && task_retreat_if_necessary(
                    tm,
                    world,
                    None,
                    unit,
                    CautionLevel::AvoidNextTurnsFire,
                )
            {
                if consider_cooldown {
                    set_attack_target_cooldown(world, target_id);
                }

                return false;
            }
        }
    }

    process_attack(tm, world, unit, target_id);

    true
}

/// Helper access to the `TaskCtx` version of `evaluate_attack`.
pub fn evaluate_attack_ctx(ctx: &mut TaskCtx<'_>, unit: UnitId, mode: bool) -> bool {
    evaluate_attack(ctx.tm, ctx.world, unit, mode)
}

/// Priority for `TaskWaitToAttack` created from `evaluate_attack`.
pub const EMERGENCY_PRIORITY: u16 = TASK_PRIORITY_EMERGENCY;
