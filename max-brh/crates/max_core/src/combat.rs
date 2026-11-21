//! Бой: урон, выстрелы, площадная атака, реактивный огонь, стелс.
//!
//! Оракул: `max-cpp-port/src/` — `unitinfo.cpp` (`UnitInfo::AttackUnit`,
//! `Attack`, `GetAreaAttackTargets`, `AttackAreaTargets`, `GetAttackRange`,
//! `PrepareFire`, `ProgressFire`, `AimAtTarget`, `InitStealthStatus`,
//! `SpotByTeam`, `GainExperience`), `units_manager.cpp`
//! (`UnitsManager_GetAttackDamage`, `AssessAttacks`, `CheckDelayedReactions`,
//! `CheckReaction`, `IsReactionPending`, `IsTeamReactionPending`,
//! `IsAttackScheduled`, `AddToDelayedReactionList`, `ClearDelayedReactions`,
//! `PinAttackTargets`, `EndAttack`, `GetStealthChancePercentage`,
//! `IsValidStealthTarget`, `AttemptStealthAction`, `CaptureUnit`,
//! `DisableUnit`, `GetTargetAngle`, `GetFiringAngle`, `ShouldAttack`),
//! `access.cpp` (`GetAreaAttackTargets`-зависимости), `zonewalker.cpp`.
//!
//! # Арифметика
//!
//! Все величины — целые, как в C++ (DESIGN §3.1/§3.5):
//! `attack_damage = max(1, attacker_attack - target_armor)`, при
//! `attack_potential > 1` атака масштабируется `((5 - potential) * attack) / 4`
//! (деление со знаком усекает к нулю — совпадает с C++), урон по подлодке от
//! бомбардировщика/чужого самолёта делится на 2. Обновления `u8/u16`
//! (`ammo`, `shots`, `hits`, `recoil`) — `wrapping_*`/насыщение там, где C++
//! заворачивается (DESIGN §3.5). Деления на 0 (нулевые `turns`/`speed` в
//! формулах опыта/отключения — риск 15) защищены ветками с документированным
//! результатом.
//!
//! # Стелс
//!
//! `UnitInfo::stealth_dice_roll` приходит пакетом 08 (P3) или генерируется
//! ИИ (`aiattack.cpp:421`: `Randomizer_Generate(101)`) — в `max_core` для этого
//! есть [`roll_stealth_dice`]. Бросок сравнивается с
//! [`get_stealth_chance_percentage`] в [`attempt_stealth_action`]; RNG при этом
//! **не** вызывается (значение уже в поле), поэтому порядок вызовов RNG
//! симуляции не меняется.
//!
//! # Отложенные реакции
//!
//! В C++ `UnitsManager_AssessAttacks` синхронизирует отложенные реакции между
//! пирами (`Remote_DelayedReactionsSync*`). `max_core` — lockstep-симуляция без
//! сети (DESIGN §1.3), поэтому портирована локальная часть: перебор команд
//! [`crate::game::GameState::delayed_reactions_team`], `CheckDelayedReactions`
//! и `IsAttackScheduled`; ветки `TEAM_TYPE_REMOTE` заменены локальным
//! поведением.

use crate::access;
use crate::enums::unit_flags::{
    FIRES_MISSILES, HAS_FIRING_SPRITE, HOVERING, MISSILE_UNIT, SELECTABLE, TURRET_SPRITE,
};
use crate::enums::{PlayMode, UnitOrder, UnitOrderState, UnitType};
use crate::game::{GameState, PLAYER_TEAM_COUNT, TEAM_SLOTS};
use crate::geo::Point;
use crate::team::{TeamId, TeamKind};
use crate::unit::{UnitId, UnitInfo, UnitListKind};
use crate::unit_order;
use crate::unit_values::{
    ATTRIB_ATTACK, ATTRIB_ATTACK_RADIUS, ATTRIB_HITS, ATTRIB_MOVE_AND_FIRE, ATTRIB_RANGE,
    ATTRIB_TURNS,
};

/// Минимальный урон `UnitsManager_GetAttackDamage` (броня не пробита).
pub const ATTACK_DAMAGE_MIN: i32 = 1;

// ---------------------------------------------------------------------------
// Урон
// ---------------------------------------------------------------------------

/// `UnitsManager_GetAttackDamage` (`units_manager.cpp:4958`) — точный порт.
///
/// `attack_potential` — дистанция площадной атаки (`0` для обычного выстрела).
pub fn get_attack_damage(attacker: &UnitInfo, target: &UnitInfo, attack_potential: i32) -> i32 {
    let target_armor = target
        .base_values
        .get_attribute(crate::unit_values::ATTRIB_ARMOR);
    let mut attacker_attack = attacker.base_values.get_attribute(ATTRIB_ATTACK);

    if attack_potential > 1 {
        attacker_attack = ((5 - attack_potential) * attacker_attack) / 4;
    }

    if target.unit_type == UnitType::Submarine
        && (attacker.unit_type == UnitType::Bomber || attacker.unit_type == UnitType::AlienPlane)
    {
        attacker_attack /= 2;
    }

    if target_armor >= attacker_attack {
        ATTACK_DAMAGE_MIN
    } else {
        attacker_attack - target_armor
    }
}

/// `UnitInfo::AttackUnit` (`unitinfo.cpp:1556`) — применяет урон к цели
/// `target_id`, атакующий — `attacker_id`.
///
/// Это метод **цели** в C++ (вызов `target->AttackUnit(attacker, …)`).
/// UI-сообщения и ИИ-уведомления (`GameManager_NotifyEvent`,
/// `Ai_AddUnitToTrackerList`, `Ai_UnitSpotted`) не портируются; видимость
/// обновляется через [`spot_unit`].
pub fn attack_unit(
    state: &mut GameState,
    target_id: UnitId,
    attacker_id: UnitId,
    attack_potential: i32,
    direction: i32,
) {
    let Some(mut target) = state.unit(target_id).cloned() else {
        return;
    };
    let Some(attacker) = state.unit(attacker_id).cloned() else {
        return;
    };

    let mut attack_damage = get_attack_damage(&attacker, &target, attack_potential);

    if target.hits == 0 {
        return;
    }

    target.damaged_this_turn = true;

    if !attacker.is_visible_to_team(target.team) {
        spot_unit(state, attacker_id, target.team);
    }

    if attack_damage > i32::from(target.hits) {
        attack_damage = i32::from(target.hits);
    }

    target.hits = u16::try_from((i32::from(target.hits) - attack_damage).max(0)).unwrap_or(0);

    if target.hits > 0 {
        // `ScheduleDelayedTasks(true)` и `Ai_AddUnitToTrackerList` (`ai.cpp:182`):
        // у команды-компьютера появляется проверка реакции, а верхняя задача
        // юнита получает напоминание о завершении состояния.
        state.write_unit(target_id, target.clone());
        // C++ `UnitInfo::AttackUnit` (`unitinfo.cpp:1575`): трекер реакции —
        // только у компьютерной команды цели. У `PLAYER_TEAM_ALIEN` нет
        // `AiPlayer` (`team_type == TEAM_TYPE_NONE`), поэтому пропускаем.
        if (target.team.0 as usize) < PLAYER_TEAM_COUNT
            && state.teams[target.team.0 as usize].team_type == TeamKind::Computer
        {
            crate::ai::ai_add_unit_to_tracker_list(state, target_id);
        }
    }

    if target.hits == 0 {
        state.remove_delayed_attack_target(target.team, target.id);
        state.remove_pending_attack(target.id);

        // `RemoveTasks`/`RemoveDelayedTasks`/`Ai_RemoveUnit` (`ai.cpp:295`).
        if target.unit_type == UnitType::Infantry || target.unit_type == UnitType::Infiltrator {
            target.angle = ((direction + 5) & 0x07) as u8;
        }
    }

    unit_order::check_if_destroyed(&mut target);

    if target.hits == 0
        || (target.orders != UnitOrder::Explode && target.state != UnitOrderState::Destroy)
    {
        unit_order::set_new_order_int(&mut target, UnitOrder::Explode, UnitOrderState::Init);
    }

    // Опыт атакующего: `enemy->GainExperience(...)` в C++ (см. `GainExperience`).
    let damaged_unit_turns = i32::from(attacker.base_values.get_attribute(ATTRIB_TURNS));
    let attacker_turns = i32::from(target.base_values.get_attribute(ATTRIB_TURNS));
    let damaged_unit_max_hits = i32::from(target.base_values.get_attribute(ATTRIB_HITS));
    state.write_unit(target_id, target);

    let denominator = attacker_turns * damaged_unit_max_hits;
    if denominator != 0 {
        let gain = (damaged_unit_turns * attack_damage * 5) / denominator;
        gain_experience(state, attacker_id, gain);
    }
}

/// `UnitInfo::GainExperience` (`unitinfo.cpp:1468`) — портирована ветка
/// `REGENERATING_UNIT` (alien-юниты); апгрейд-темы (`Randomizer_Generate`) —
/// P3/P6, отмечено TODO(риск 1).
pub fn gain_experience(state: &mut GameState, id: UnitId, gain: i32) {
    let Some(mut unit) = state.unit(id).cloned() else {
        return;
    };

    if unit.flags & crate::enums::unit_flags::REGENERATING_UNIT == 0 {
        return;
    }

    unit.experience = unit.experience.wrapping_add(gain as i16);

    // TODO(риск 1/P6): цикл `while (upgrade_cost <= experience)` с
    // `ExpResearchTopics[Randomizer_Generate(sizeof)]` — research-таблицы.
    state.write_unit(id, unit);
}

/// `Ai_UnitSpotted`/`UnitInfo::SpotByTeam` — минимальная симуляционная часть:
/// юнит становится видимым и «замеченным» для команды.
pub fn spot_unit(state: &mut GameState, id: UnitId, team: TeamId) {
    let Some(mut unit) = state.unit(id).cloned() else {
        return;
    };
    unit.set_visible_to_team(team, true);
    unit.spot_by_team(team);
    state.write_unit(id, unit);
}

// ---------------------------------------------------------------------------
// Углы
// ---------------------------------------------------------------------------

/// `UnitsManager_GetTargetAngle` (`units_manager.cpp:1711`).
pub fn get_target_angle(distance_x: i32, distance_y: i32) -> i32 {
    let level_x = distance_x.abs();
    let level_y = distance_y.abs();

    if distance_x > 0 || distance_y > 0 {
        if distance_x >= 0 && distance_y >= 0 {
            if level_y / 2 > level_x || level_x == 0 {
                4
            } else if level_x / 2 > level_y || level_y == 0 {
                2
            } else {
                3
            }
        } else if distance_x > 0 {
            if level_y / 2 > level_x || level_x == 0 {
                0
            } else if level_x / 2 > level_y || level_y == 0 {
                2
            } else {
                1
            }
        } else if level_y / 2 > level_x || level_x == 0 {
            4
        } else if level_x / 2 > level_y || level_y == 0 {
            6
        } else {
            5
        }
    } else if level_y / 2 > level_x || level_x == 0 {
        0
    } else if level_x / 2 > level_y || level_y == 0 {
        6
    } else {
        7
    }
}

/// `UnitsManager_GetFiringAngle` (`units_manager.cpp:1767`) — 0..15
/// (направление спрайта выстрела).
pub fn get_firing_angle(distance_x: i32, distance_y: i32) -> i32 {
    let level_x = distance_x.abs();
    let level_y = distance_y.abs();

    if distance_x > 0 || distance_y > 0 {
        if distance_x >= 0 && distance_y >= 0 {
            if level_y / 4 > level_x || level_x == 0 {
                8
            } else if level_x / 4 > level_y || level_y == 0 {
                4
            } else if level_y / 2 >= level_x {
                7
            } else if level_x / 2 >= level_y {
                5
            } else {
                6
            }
        } else if distance_x > 0 {
            if level_y / 4 > level_x || level_x == 0 {
                0
            } else if level_x / 4 > level_y || level_y == 0 {
                4
            } else if level_y / 2 >= level_x {
                1
            } else if level_x / 2 >= level_y {
                3
            } else {
                2
            }
        } else if level_y / 4 > level_x || level_x == 0 {
            8
        } else if level_x / 4 > level_y || level_y == 0 {
            12
        } else if level_y / 2 >= level_x {
            9
        } else if level_x / 2 >= level_y {
            11
        } else {
            10
        }
    } else if level_y / 4 > level_x || level_x == 0 {
        0
    } else if level_x / 4 > level_y || level_y == 0 {
        12
    } else if level_y / 2 >= level_x {
        15
    } else if level_x / 2 >= level_y {
        13
    } else {
        14
    }
}

// ---------------------------------------------------------------------------
// Площадная атака (ZoneWalker + GetAreaAttackTargets)
// ---------------------------------------------------------------------------

/// Обход клеток в круге радиуса `range` — `ZoneWalker` (`zonewalker.cpp`):
/// строки по `y` возрастанию, внутри строки — `x` возрастанию; клетки вне
/// круга не посещаются, края обрезаются картой.
pub fn zone_cells(grid: Point, range: i32) -> Vec<Point> {
    let mut result = Vec::new();
    if range < 0 {
        return result;
    }

    let start_x = i32::from(grid.x);
    let start_y = i32::from(grid.y);
    let distance = range * range;

    let mut y = start_y - range;
    if y < 0 {
        y = 0;
    }
    let limit_y = (start_y + range).min(i32::from(crate::geo::MAP_HEIGHT) - 1);

    while y <= limit_y {
        let distance_y = (y - start_y) * (y - start_y);

        let mut x = start_x - range;
        while x <= start_x {
            let distance_x = (x - start_x) * (x - start_x);
            if distance_x + distance_y <= distance {
                break;
            }
            x += 1;
        }

        let limit_x = (start_x * 2 - x).min(i32::from(crate::geo::MAP_WIDTH) - 1);
        let mut x = x.max(0);

        while x <= limit_x {
            if (x - start_x) * (x - start_x) + distance_y <= distance {
                result.push(Point::new(x as i16, y as i16));
            }
            x += 1;
        }

        y += 1;
    }

    result
}

/// `UnitInfo::GetAreaAttackTargets` (`unitinfo.cpp:3919`): уникальные цели в
/// радиусе, с минимальной `Access_GetApproximateDistance` до центра.
pub fn get_area_attack_targets(
    state: &GameState,
    unit: &UnitInfo,
    grid: Point,
) -> Vec<(UnitId, i32)> {
    let radius = unit.base_values.get_attribute(ATTRIB_ATTACK_RADIUS);
    let mut targets: Vec<(UnitId, i32)> = Vec::new();

    for location in zone_cells(grid, radius) {
        let Some(ids) = state.units_at_opt(location) else {
            continue;
        };
        let distance = access::approximate_distance_points(location, grid);

        for id in ids.iter().copied() {
            let Some(other) = state.unit(id) else {
                continue;
            };

            if other.flags & SELECTABLE == 0
                || other.orders == UnitOrder::Idle
                || !access::is_valid_attack_target_point(state, unit, other, grid)
            {
                continue;
            }

            if (other.unit_type == UnitType::Bridge || other.unit_type == UnitType::WaterPlatform)
                && access::is_unit_busy_at_location(state, other)
            {
                continue;
            }

            match targets.iter_mut().find(|(target, _)| *target == id) {
                Some(entry) => {
                    if distance < entry.1 {
                        entry.1 = distance;
                    }
                }
                None => targets.push((id, distance)),
            }
        }
    }

    targets
}

/// `UnitInfo::AttackAreaTargets` (`unitinfo.cpp:3961`).
pub fn attack_area_targets(state: &mut GameState, attacker_id: UnitId, grid: Point) {
    let Some(attacker) = state.unit(attacker_id).cloned() else {
        return;
    };

    for (target, distance) in get_area_attack_targets(state, &attacker, grid) {
        let Some(target_unit) = state.unit(target).cloned() else {
            continue;
        };
        let direction = get_target_angle(
            i32::from(target_unit.grid.x) - i32::from(grid.x),
            i32::from(target_unit.grid.y) - i32::from(grid.y),
        );
        attack_unit(state, target, attacker_id, distance, direction);
    }
}

// ---------------------------------------------------------------------------
// Выстрел (UnitInfo::Attack)
// ---------------------------------------------------------------------------

/// `UnitInfo::Attack` (`unitinfo.cpp:2700`) — выстрел по клетке.
///
/// Вызывается из `ProgressFire` после `EndAttack` (не из приказа FIRE).
pub fn attack(state: &mut GameState, id: UnitId, grid: Point) {
    let Some(unit) = state.unit(id).cloned() else {
        return;
    };

    let mut fire_target = access::get_attack_target(state, &unit, grid, false);

    if fire_target.is_none() {
        if let Some(target) = access::get_attack_target2(state, &unit, grid) {
            if let Some(target_unit) = state.unit(target) {
                let team = target_unit.team;
                let _ = team;
            }
            // `target->SetVisibleToTeam(team, true)` в C++.
            if let Some(mut target_unit) = state.unit(target).cloned() {
                target_unit.set_visible_to_team(unit.team, true);
                state.write_unit(target, target_unit);
            }
            fire_target = Some(target);
        } else {
            let enemy = access::get_team_unit(state, grid, unit.team, SELECTABLE);
            if enemy.is_none()
                && unit.unit_type != UnitType::AntiAir
                && unit.unit_type != UnitType::MobileAntiAir
                && unit.unit_type != UnitType::Escort
            {
                let flags = access::unit_type_flags(UnitType::HitExplosion);
                state.spawn_unit(UnitType::HitExplosion, unit.team, grid, flags);
            } else if let Some(enemy_id) = enemy {
                if let Some(enemy_unit) = state.unit(enemy_id) {
                    if access::is_valid_attack_target_point(state, &unit, enemy_unit, grid) {
                        fire_target = Some(enemy_id);
                    }
                }
            }
        }
    }

    let target_angle = if unit.flags & MISSILE_UNIT != 0 {
        i32::from(unit.angle) / 2
    } else {
        get_target_angle(
            i32::from(grid.x) - i32::from(unit.grid.x),
            i32::from(grid.y) - i32::from(unit.grid.y),
        )
    };

    if unit.base_values.get_attribute(ATTRIB_ATTACK_RADIUS) != 0 {
        attack_area_targets(state, id, grid);
    } else if let Some(target) = fire_target {
        attack_unit(state, target, id, 0, target_angle);
    }

    if unit.shots == 0 {
        if let Some(unit) = state.unit_mut(id) {
            unit.targeting_mode = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// PrepareFire / ProgressFire / AimAtTarget
// ---------------------------------------------------------------------------

/// `UnitInfo::PrepareFire` (`unitinfo.cpp:4709`).
///
/// Списывает боезапас, при `FIRES_MISSILES` создаёт снаряд и переносит на него
/// `live_attack`, переводит приказ в `FIRE_IN_PROGRESS`.
pub fn prepare_fire(state: &mut GameState, unit: &mut UnitInfo) {
    unit.firing_recoil_frames = 3;
    if unit.unit_type == UnitType::AntiAir {
        unit.firing_recoil_frames *= 5;
    } else if unit.unit_type == UnitType::MobileAntiAir || unit.unit_type == UnitType::Escort {
        unit.firing_recoil_frames *= 2;
    }

    if unit.flags & HAS_FIRING_SPRITE != 0
        && (unit.unit_type == UnitType::Infiltrator || unit.unit_type == UnitType::Infantry)
    {
        unit.firing_recoil_frames = 8;
    }

    // `UnitInfo::PrepareFire` (`unitinfo.cpp:4718-4726`):
    // `PlaySfx(this, Unit::SFX_TYPE_FIRE)` — выстрел орудия юнита. C++ играет
    // его лишь при видимости цели для локальной команды (`team_visibility`) или
    // по heat map; `GameManager_PlayerTeam` — UI-поле, поэтому в lockstep
    // симуляции событие копится всегда (DESIGN §6.1), а фильтрует его клиент.
    state.unit_sounds.push(crate::game::UnitSound {
        unit: unit.id,
        unit_type: unit.unit_type,
        kind: crate::game::UnitSoundKind::Fire,
    });

    if unit.flags & FIRES_MISSILES != 0 {
        let particle_unit = match unit.unit_type {
            UnitType::Submarine | UnitType::Corvette => UnitType::Torpedo,
            UnitType::AlienTank => UnitType::AlienTankBall,
            UnitType::AlienAssaultGun | UnitType::Juggernaut => UnitType::AlienArtilleryBall,
            UnitType::AlienPlane => UnitType::AlienMissile,
            _ => UnitType::Rocket,
        };

        let fire_on = state.order_data(unit.id).fire_on;
        let angle = if particle_unit == UnitType::AlienTankBall
            || particle_unit == UnitType::AlienArtilleryBall
        {
            0
        } else {
            let firing_angle = (get_firing_angle(
                i32::from(fire_on.x) - i32::from(unit.grid.x),
                i32::from(fire_on.y) - i32::from(unit.grid.y),
            ) & 0x0F) as u8;
            if particle_unit == UnitType::AlienMissile {
                firing_angle.wrapping_mul(2)
            } else {
                firing_angle
            }
        };

        let flags = access::unit_type_flags(particle_unit);
        if let Some(particle_id) = state.spawn_unit(particle_unit, unit.team, unit.grid, flags) {
            let mut data = state.order_data(particle_id);
            data.move_to = fire_on;
            data.fire_on = fire_on;
            state.set_order_data(particle_id, data);

            if let Some(mut particle) = state.unit(particle_id).cloned() {
                particle.angle = angle & 0x07;
                particle.target_grid = fire_on;
                particle.parent_unit = Some(unit.id);
                particle.live_attack = unit.live_attack;
                particle.orders = UnitOrder::Move;
                particle.state = UnitOrderState::Init;
                state.write_unit(particle_id, particle);
            }

            unit.live_attack = 0;
        }

        state.add_to_delayed_reaction_list(unit.id);
    }

    unit.ammo = unit.ammo.wrapping_sub(1);

    if state.real_time {
        if unit.shots > unit.ammo {
            unit.shots = unit.ammo;
        }
    } else {
        unit.shots = unit.shots.wrapping_sub(1);
    }

    unit.state = UnitOrderState::FireInProgress;
}

/// `UnitInfo::ProgressFire` (`unitinfo.cpp:4788`).
///
/// Возвращает `true`, когда отдача закончилась и пора наносить урон (C++ в
/// этот момент вызывает `Attack(fire_on)`); `false` — анимация ещё идёт.
pub fn progress_fire(state: &mut GameState, unit: &mut UnitInfo) -> bool {
    unit.firing_recoil_frames = unit.firing_recoil_frames.wrapping_sub(1);

    if unit.firing_recoil_frames != 0 {
        return false;
    }

    unit_order::restore_orders(unit);

    if unit.flags & FIRES_MISSILES == 0 {
        state.end_attack(unit.id);
        unit.live_attack = 0;
        state.add_to_delayed_reaction_list(unit.id);
        return true;
    }

    false
}

/// `UnitInfo::AimAtTarget` (`unitinfo.cpp:5207`).
///
/// Поворачивает корпус/башню на один шаг к `fire_on`; `true`, когда угол
/// совпал (приказ переходит в `READY_TO_FIRE`).
pub fn aim_at_target(state: &GameState, unit: &mut UnitInfo, fire_on: Point) -> bool {
    let unit_angle = if unit.flags & TURRET_SPRITE != 0 {
        i32::from(unit.turret_angle)
    } else {
        i32::from(unit.angle)
    };

    let distance_x = i32::from(fire_on.x) - i32::from(unit.grid.x);
    let distance_y = i32::from(fire_on.y) - i32::from(unit.grid.y);
    let target_angle = get_target_angle(distance_x, distance_y);

    if unit_angle == target_angle {
        unit.state = UnitOrderState::ReadyToFire;

        // C++ `SpotByTeam` для команд, у которых открыта клетка в heat map
        // (`HeatMap::GetComplete`): покрытие сканирования — P6.
        for team_index in 0..PLAYER_TEAM_COUNT {
            let team = TeamId(team_index as u8);
            if unit.team != team
                && state.teams[team_index].team_type != TeamKind::None
                && state.ai_heat_map_complete(team, i32::from(unit.grid.x), i32::from(unit.grid.y))
                    > 0
            {
                unit.spot_by_team(team);
            }
        }

        true
    } else {
        let direction = if ((target_angle - unit_angle + 8) & 0x07) > 4 {
            -1
        } else {
            1
        };

        let angle = ((unit_angle + direction) & 0x07) as u8;

        if unit.flags & TURRET_SPRITE != 0 {
            unit.turret_angle = angle;
        } else {
            unit.angle = angle;
        }

        false
    }
}

// ---------------------------------------------------------------------------
// Реактивный огонь (AssessAttacks)
// ---------------------------------------------------------------------------

/// `UnitsManager_ShouldAttack` (`units_manager.cpp:4623`).
pub fn should_attack(state: &GameState, unit1: &UnitInfo, unit2: &UnitInfo) -> bool {
    if (unit1.unit_type == UnitType::MobileAntiAir
        || unit1.unit_type == UnitType::AntiAir
        || unit1.unit_type == UnitType::Escort)
        && !unit2.is_air()
    {
        return false;
    }

    if unit2.hits == 0 {
        return false;
    }

    if unit1.orders == UnitOrder::Idle
        || (unit1.has_move_order() && unit1.state != UnitOrderState::ExecutingOrder)
        || unit1.orders == UnitOrder::Disable
        || unit1.state == UnitOrderState::Explode
    {
        return false;
    }

    if !unit2.is_visible_to_team(unit1.team) {
        return false;
    }

    if (unit1.unit_type == UnitType::Infiltrator || unit1.unit_type == UnitType::Submarine)
        && !unit1.is_visible_to_team(unit2.team)
    {
        return false;
    }

    if state.teams[unit1.team.0 as usize].team_type == TeamKind::Computer {
        // Компьютер считает враждебной любую другую команду.
        return true;
    }

    if state.play_mode == PlayMode::TurnBased || unit1.orders == UnitOrder::Sentry {
        return true;
    }

    if state.teams[unit1.team.0 as usize].finished_turn && state.play_mode == PlayMode::Unknown {
        return true;
    }

    if unit2.shots > 0 && !unit1.disabled_reaction_fire {
        let unit1_team = unit1.team;
        let mut unit2_distance = unit2.base_values.get_attribute(ATTRIB_RANGE);
        unit2_distance *= unit2_distance;

        for kind in [
            UnitListKind::MobileLandSea,
            UnitListKind::MobileAir,
            UnitListKind::Stationary,
        ] {
            for id in state.unit_list(kind) {
                let Some(other) = state.unit(*id) else {
                    continue;
                };
                if other.team == unit1_team
                    && access::squared_distance(unit2.grid, other.grid) <= unit2_distance
                    && access::is_valid_attack_target(state, unit2, other)
                {
                    return true;
                }
            }
        }

        return false;
    }

    false
}

/// `UnitsManager_CheckReaction` (`units_manager.cpp:4709`): цель в радиусе и
/// достойна атаки — выдать `ORDER_FIRE`.
pub fn check_reaction(state: &mut GameState, unit1_id: UnitId, unit2_id: UnitId) -> bool {
    let Some(unit1) = state.unit(unit1_id).cloned() else {
        return false;
    };
    let Some(unit2) = state.unit(unit2_id).cloned() else {
        return false;
    };

    if unit1.shots == 0
        || unit1.delayed_reaction != 0
        || !access::is_within_attack_range(
            state,
            &unit1,
            unit2.grid,
            unit1.base_values.get_attribute(ATTRIB_RANGE),
        )
        || !access::is_valid_attack_target(state, &unit1, &unit2)
        || !should_attack(state, &unit1, &unit2)
    {
        return false;
    }

    let mut data = state.order_data(unit1_id);
    data.fire_on = unit2.grid;
    state.set_order_data(unit1_id, data);
    state.set_order(unit1_id, UnitOrder::Fire, UnitOrderState::Init);
    true
}

/// `UnitsManager_IsReactionPending` (`units_manager.cpp:4749`): есть ли в
/// списке юнит, готовый отреагировать на `unit`.
pub fn is_reaction_pending(state: &mut GameState, owner: TeamId, unit_id: UnitId) -> bool {
    let candidates = state
        .delayed_attack_targets
        .get(owner.0 as usize)
        .cloned()
        .unwrap_or_default();

    for candidate in candidates {
        if check_reaction(state, candidate, unit_id) {
            return true;
        }
    }

    false
}

/// `UnitsManager_IsTeamReactionPending` (`units_manager.cpp:4608`).
fn is_team_reaction_pending(
    state: &mut GameState,
    team: TeamId,
    unit_id: UnitId,
    kind: UnitListKind,
) -> bool {
    let ids = state.unit_list(kind).to_vec();
    for id in ids {
        let Some(unit) = state.unit(id) else { continue };
        if unit.team == team && check_reaction(state, id, unit_id) {
            return true;
        }
    }
    false
}

/// `UnitsManager_CheckDelayedReactions(team)` (`units_manager.cpp:4888`).
pub fn check_delayed_reactions(state: &mut GameState, team: TeamId) -> bool {
    // C++ (`units_manager.cpp:4891`): реактивный огонь только у команд с
    // `team_type != TEAM_TYPE_NONE`. У `PLAYER_TEAM_ALIEN` тип — `NONE`, а в
    // max_core слот помечен `Computer`, поэтому проверяем индекс явно:
    // alien-юниты не должны получать реактивные приказы.
    if team.0 as usize >= PLAYER_TEAM_COUNT {
        return false;
    }

    let team_type = state.teams[team.0 as usize].team_type;
    if team_type == TeamKind::None || team_type == TeamKind::Eliminated {
        return false;
    }

    if let Some(unit1_id) = state.units_active[team.0 as usize] {
        let Some(unit1) = state.unit(unit1_id).cloned() else {
            return false;
        };
        let Some(unit2_id) = unit1.enemy_unit else {
            return false;
        };
        let Some(unit2) = state.unit(unit2_id).cloned() else {
            return false;
        };

        let position = access::get_attack_position(state, &unit1, &unit2);

        let mut data = state.order_data(unit1_id);
        data.fire_on = position;
        state.set_order_data(unit1_id, data);

        if state.teams[unit1.team.0 as usize].team_type == TeamKind::Computer
            && unit1.base_values.get_attribute(ATTRIB_MOVE_AND_FIRE) != 0
            && unit1.shots == 1
        {
            state.set_order(unit1_id, UnitOrder::Await, UnitOrderState::ExecutingOrder);
            if let Some(mut unit) = state.unit(unit1_id).cloned() {
                unit.enemy_unit = None;
                state.write_unit(unit1_id, unit);
            }
        }

        state.set_order(unit1_id, UnitOrder::Fire, UnitOrderState::Init);
        return true;
    }

    for other in 0..PLAYER_TEAM_COUNT {
        let other_team = TeamId(other as u8);
        if team == other_team {
            continue;
        }

        let targets = state.delayed_attack_targets[other].clone();
        for target in &targets {
            let visible = state
                .unit(*target)
                .map(|unit| unit.is_visible_to_team(team))
                .unwrap_or(false);
            if visible && is_reaction_pending(state, team, *target) {
                return true;
            }
        }

        for target in &targets {
            let visible = state
                .unit(*target)
                .map(|unit| unit.is_visible_to_team(team))
                .unwrap_or(false);
            if !visible {
                continue;
            }

            if is_team_reaction_pending(state, team, *target, UnitListKind::MobileLandSea)
                || is_team_reaction_pending(state, team, *target, UnitListKind::MobileAir)
                || is_team_reaction_pending(state, team, *target, UnitListKind::Stationary)
            {
                return true;
            }
        }
    }

    false
}

/// `UnitsManager_IsAttackScheduled` (`units_manager.cpp:4822`).
pub fn is_attack_scheduled(state: &GameState) -> bool {
    for kind in [
        UnitListKind::MobileAir,
        UnitListKind::MobileLandSea,
        UnitListKind::Stationary,
    ] {
        for id in state.unit_list(kind) {
            if let Some(unit) = state.unit(*id) {
                if unit.orders == UnitOrder::Fire {
                    return true;
                }
            }
        }
    }
    false
}

/// `UnitsManager_AssessAttacks` (`units_manager.cpp:4490`) — локальная часть
/// без сетевой синхронизации отложенных реакций.
pub fn assess_attacks(state: &mut GameState) -> bool {
    if state.disable_fire {
        return false;
    }

    let mut are_attacks_delayed = false;

    for team in 0..TEAM_SLOTS {
        let has_pending =
            !state.delayed_attack_targets[team].is_empty() || state.units_active[team].is_some();

        if state.teams[team].team_type == TeamKind::Eliminated {
            continue;
        }

        if has_pending {
            are_attacks_delayed = true;
        }
    }

    if !are_attacks_delayed {
        state.delayed_reactions_pending = false;
        return false;
    }

    // C++ обходит команды, начиная с `UnitsManager_DelayedReactionsTeam`, и
    // возвращает `true` на первой сработавшей реакции; иначе очищает состояние.
    let start = state.delayed_reactions_team.0;
    let mut team = start;

    loop {
        let triggered = check_delayed_reactions(state, TeamId(team));
        team = (team + 1) % crate::game::PLAYER_TEAM_MAX;
        state.delayed_reactions_team = TeamId(team);

        if triggered {
            return true;
        }

        if team == start {
            break;
        }
    }

    state.clear_delayed_reactions();
    is_attack_scheduled(state)
}

// ---------------------------------------------------------------------------
// Стелс-действия (диверсант)
// ---------------------------------------------------------------------------

/// `UnitsManager_GetStealthChancePercentage` (`units_manager.cpp:1354`).
pub fn get_stealth_chance_percentage(
    state: &GameState,
    unit: &UnitInfo,
    target: &UnitInfo,
    order: UnitOrder,
) -> i32 {
    let unit_turns = state
        .team_units
        .get(target.team.0 as usize)
        .map(|values| {
            values
                .current_unit_values(target.unit_type as u16)
                .get_attribute(ATTRIB_TURNS)
        })
        .unwrap_or(0);

    if unit_turns == 0 {
        // C++ делит на `turns` (риск 15): нулевые ходы недостижимы у игровых
        // типов; ветка страхует деление на 0.
        return 90;
    }

    let mut chance = (i32::from(unit.experience) * 12 * 8 + 640) / unit_turns;

    if order == UnitOrder::AwaitStealUnit {
        chance /= 4;
    }

    chance.min(90)
}

/// `Task_IsAdjacent` (см. `access::is_adjacent_to_grid`).
fn is_adjacent(unit: &UnitInfo, target: &UnitInfo) -> bool {
    access::is_adjacent_to_grid(unit, target.grid)
}

/// `UnitsManager_IsValidStealthTarget` (`units_manager.cpp:1370`).
pub fn is_valid_stealth_target(
    state: &GameState,
    unit: &UnitInfo,
    target: &UnitInfo,
    order: UnitOrder,
) -> bool {
    let target_order = target.orders;

    if target.team == unit.team
        || target.hits == 0
        || target.flags & crate::enums::unit_flags::ELECTRONIC_UNIT == 0
        || (target.is_air() && target.flags & HOVERING != 0)
        || target_order == UnitOrder::Idle
        || (target_order == UnitOrder::AwaitScaling && target.prior_orders == UnitOrder::Idle)
        || target_order == UnitOrder::Transform
        || target_order == UnitOrder::Fire
        || target_order == UnitOrder::Explode
        || target.state == UnitOrderState::Destroy
        || !is_adjacent(unit, target)
        || !access::is_valid_attack_target(state, unit, target)
    {
        return false;
    }

    if order == UnitOrder::AwaitStealUnit {
        target.is_mobile()
            && !((target.unit_type == UnitType::ArmouredPersonnelCarrier
                || target.unit_type == UnitType::SeaTransport
                || target.unit_type == UnitType::AirTransport)
                && target.storage != 0)
    } else {
        target_order != UnitOrder::Disable
    }
}

/// `Randomizer_Generate(101)` для `stealth_dice_roll` (`aiattack.cpp:421`).
///
/// Используется ИИ-слоем (P6) и тестами; сама атака RNG не потребляет.
pub fn roll_stealth_dice(state: &mut GameState) -> u8 {
    state.rng.generate_i64(101) as u8
}

/// `UnitsManager_AttemptStealthAction` (`units_manager.cpp:4254`).
///
/// Возвращает `true`, если действие удалось. `unit` — диверсант (его
/// `parent_unit` — цель). Результат (захват/отключение) применяет вызывающий
/// слой ([`capture_unit`]/[`disable_unit`]), как в `UnitsManager_ProcessOrder`.
pub fn attempt_stealth_action(state: &mut GameState, unit: &mut UnitInfo) -> bool {
    let Some(parent_id) = unit.parent_unit else {
        return false;
    };
    let Some(parent) = state.unit(parent_id).cloned() else {
        return false;
    };

    let stealth_chance = get_stealth_chance_percentage(state, unit, &parent, unit.orders);

    if !is_valid_stealth_target(state, unit, &parent, unit.orders) {
        return false;
    }

    if !state.real_time {
        unit.shots = unit.shots.wrapping_sub(1);
    }

    if i32::from(unit.stealth_dice_roll) <= stealth_chance {
        if parent.orders != UnitOrder::Disable {
            unit.experience = unit.experience.wrapping_add(1);
        }
        true
    } else if parent.orders == UnitOrder::Disable {
        false
    } else {
        // Провал: диверсант раскрыт, цель/её команда его видит.
        unit.spot_by_team(parent.team);
        unit.set_visible_to_team(parent.team, true);
        state.add_to_delayed_reaction_list(unit.id);
        false
    }
}

/// `UnitsManager_CaptureUnit` (`units_manager.cpp:4339`): цель переходит под
/// контроль команды диверсанта.
pub fn capture_unit(state: &mut GameState, unit_id: UnitId, parent_id: UnitId) {
    let (new_team, parent_order) = match (state.unit(unit_id), state.unit(parent_id)) {
        (Some(unit), Some(parent)) => (unit.team, parent.orders),
        _ => return,
    };

    // `if (parent->GetOrder() == ORDER_BUILD || ORDER_CLEAR) { … CancelBuilding }`.
    if parent_order == UnitOrder::Build || parent_order == UnitOrder::Clear {
        if let Some(mut parent) = state.unit(parent_id).cloned() {
            unit_order::cancel_building(&mut parent);
            state.write_unit(parent_id, parent);
        }
    }

    change_team(state, parent_id, new_team);

    spot_unit(state, parent_id, new_team);
}

/// `UnitInfo::ChangeTeam` (`unitinfo.cpp`) — симуляционная часть: смена команды
/// со сбросом пути/цели/родителя/группы и видимости.
pub fn change_team(state: &mut GameState, target_id: UnitId, target_team: TeamId) {
    let Some(mut unit) = state.unit(target_id).cloned() else {
        return;
    };
    let old_team = unit.team;
    if old_team == target_team {
        return;
    }

    state.remove_path_request(target_id);
    state.remove_delayed_attack_target(old_team, target_id);
    access::clear_unit_group(state, target_id);

    unit.parent_unit = None;
    unit.enemy_unit = None;
    unit_order::stop_move(&mut unit, state.disable_fire);
    unit.path = None;
    unit.set_visible_to_team(old_team, false);
    unit.team = target_team;

    if unit.orders == UnitOrder::Disable {
        unit_order::restore_orders(&mut unit);
        unit.disabled_turns_remaining = 0;
    }

    unit.set_visible_to_team(target_team, true);
    unit.spot_by_team(target_team);
    state.write_unit(target_id, unit);
}

/// `UnitsManager_DisableUnit` (`units_manager.cpp:4395`): цель отключена на
/// `service_disable_turns` ходов; путь/приказ сбрасываются.
pub fn disable_unit(state: &mut GameState, unit_id: UnitId, parent_id: UnitId) {
    let Some(infiltrator) = state.unit(unit_id).cloned() else {
        return;
    };
    let Some(parent) = state.unit(parent_id).cloned() else {
        return;
    };

    let turns = access::disable_turns(&infiltrator, &parent);

    if let Some(mut parent) = state.unit(parent_id).cloned() {
        // `parent->StopMove()`.
        unit_order::stop_move(&mut parent, state.disable_fire);

        match parent.orders {
            UnitOrder::Build | UnitOrder::Clear => {
                unit_order::cancel_building(&mut parent);
            }
            UnitOrder::PowerOn => {
                unit_order::set_new_order_int(
                    &mut parent,
                    UnitOrder::PowerOff,
                    UnitOrderState::ExecutingOrder,
                );
                unit_order::power_down(state, &mut parent);

                if parent.unit_type == UnitType::ResearchCentre {
                    // `UnitInfo::PowerDown` (`unitinfo.cpp:5134`).
                    state.update_research_progress(
                        parent.team,
                        parent.research_topic,
                        -1,
                    );
                }
            }
            _ => {}
        }

        parent.path = None;
        unit_order::set_new_order_int(
            &mut parent,
            UnitOrder::Disable,
            UnitOrderState::ExecutingOrder,
        );
        parent.disabled_turns_remaining = turns as u8;
        state.write_unit(parent_id, parent);
    }

    if let Some(unit) = state.unit(unit_id) {
        let team = unit.team;
        spot_unit(state, parent_id, team);
    }
}

// ---------------------------------------------------------------------------
// Вспомогательное
// ---------------------------------------------------------------------------

/// `UnitInfo::GetAttackRange`-обёртка (см. [`access::get_attack_range`]).
#[inline]
pub fn get_attack_range(unit: &UnitInfo) -> i32 {
    access::get_attack_range(unit)
}

/// `UnitInfo::InitStealthStatus` (`unitinfo.cpp:2796`) — мины, диверсанты и
/// подлодки невидимы всем, остальные видны только если включён
/// `GameManager_AllVisible`; своя команда видит всегда.
pub fn init_stealth_status(unit: &mut UnitInfo, all_visible: bool) {
    let stealth = matches!(
        unit.unit_type,
        UnitType::LandMine | UnitType::SeaMine | UnitType::Infiltrator | UnitType::Submarine
    );

    for index in 0..TEAM_SLOTS {
        unit.visible_to_team[index] = !stealth && all_visible;
        unit.spotted_by_team[index] = false;
    }

    unit.set_visible_to_team(unit.team, true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::unit_flags::{ELECTRONIC_UNIT, MOBILE_LAND_UNIT, SELECTABLE, STATIONARY};
    use crate::enums::TeamClan;
    use crate::team::TeamKind;
    use crate::unit_values::{ATTRIB_ARMOR, ATTRIB_SPEED, ATTRIB_TURNS};

    fn state() -> GameState {
        let mut state = GameState::new();
        state.teams[0].team_type = TeamKind::Player;
        state.teams[1].team_type = TeamKind::Player;
        state.team_units[0] = crate::team::TeamUnits::with_clan(TeamClan::Random);
        state.team_units[1] = crate::team::TeamUnits::with_clan(TeamClan::Random);
        state
    }

    fn spawn_tank(state: &mut GameState, team: TeamId, grid: Point) -> UnitId {
        state
            .spawn_unit(
                UnitType::Tank,
                team,
                grid,
                MOBILE_LAND_UNIT | SELECTABLE | crate::enums::unit_flags::UPGRADABLE,
            )
            .unwrap()
    }

    #[test]
    fn damage_formula_matches_cpp() {
        // TANK: attack 16, TANK armor 10 → 6 урона.
        let attacker = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
        let target = UnitInfo::new(UnitType::Tank, TeamId::GREEN, UnitId(2));
        assert_eq!(
            attacker.base_values.get_attribute(ATTRIB_ATTACK),
            16,
            "атрибуты танка из таблицы SC_ATTRI"
        );
        assert_eq!(target.base_values.get_attribute(ATTRIB_ARMOR), 10);
        assert_eq!(get_attack_damage(&attacker, &target, 0), 6);

        // Броня >= атаки: минимальный урон 1.
        let mut weak = attacker.clone();
        weak.base_values.set_attribute(ATTRIB_ATTACK, 4);
        assert_eq!(get_attack_damage(&weak, &target, 0), ATTACK_DAMAGE_MIN);

        // Площадная атака: attack * (5 - potential) / 4.
        assert_eq!(get_attack_damage(&attacker, &target, 2), 2); // 16*3/4=12 → 2
        assert_eq!(get_attack_damage(&attacker, &target, 3), 1); // 16*2/4=8 → 1
        assert_eq!(get_attack_damage(&attacker, &target, 5), 1); // 0 → 1

        // Подлодка: бомбардировщик бьёт вдвое слабее.
        let bomber = UnitInfo::new(UnitType::Bomber, TeamId::RED, UnitId(3));
        let submarine = UnitInfo::new(UnitType::Submarine, TeamId::GREEN, UnitId(4));
        assert_eq!(get_attack_damage(&bomber, &submarine, 0), 7);
    }

    #[test]
    fn attack_unit_applies_damage_and_explodes_target() {
        let mut state = state();
        let red = spawn_tank(&mut state, TeamId::RED, Point::new(5, 5));
        let green = spawn_tank(&mut state, TeamId::GREEN, Point::new(6, 5));
        // Цель видна атакующему (в игре — после сканирования/разведки).
        state.update_visibility_status(false);

        // Пин живой атаки: цель получает pin_count.
        let mut data = state.order_data(red);
        data.fire_on = Point::new(6, 5);
        state.set_order_data(red, data);
        state.pin_attack_targets(red);
        assert_eq!(state.unit(green).unwrap().pin_count, 1);
        assert!(state.live_attacks[0].targets.contains(&green));

        // Урон 6 при hits 5 — цель уничтожается.
        state.end_attack(red);
        state.unit_mut(green).unwrap().hits = 5;
        state.pending_attacks.push(red);
        attack_unit(&mut state, green, red, 0, 2);

        let target = state.unit(green).unwrap();
        assert_eq!(target.hits, 0);
        assert!(target.damaged_this_turn);
        assert_eq!(target.orders, UnitOrder::Explode);
        assert_eq!(target.state, UnitOrderState::Init);
        assert!(!state.pending_attacks.contains(&green));
        assert_eq!(target.pin_count, 0);
    }

    #[test]
    fn area_attack_hits_all_targets_in_radius() {
        let mut state = state();
        let artillery = state
            .spawn_unit(
                UnitType::RocketLauncher,
                TeamId::RED,
                Point::new(10, 10),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .unwrap();
        let near = spawn_tank(&mut state, TeamId::GREEN, Point::new(11, 10));
        let far = spawn_tank(&mut state, TeamId::GREEN, Point::new(12, 10));
        let outside = spawn_tank(&mut state, TeamId::GREEN, Point::new(14, 10));

        let attacker = state.unit(artillery).unwrap().clone();
        let targets = get_area_attack_targets(&state, &attacker, Point::new(10, 10));
        assert!(targets.iter().any(|(id, _)| *id == near));
        assert!(targets.iter().any(|(id, _)| *id == far));
        assert!(!targets.iter().any(|(id, _)| *id == outside));
        // Уникальность целей.
        let unique: std::collections::BTreeSet<_> = targets.iter().map(|(id, _)| *id).collect();
        assert_eq!(unique.len(), targets.len());

        attack_area_targets(&mut state, artillery, Point::new(10, 10));
        assert_eq!(state.unit(near).unwrap().hits, 23);
        assert_eq!(state.unit(far).unwrap().hits, 23);
        assert_eq!(state.unit(outside).unwrap().hits, 24);
    }

    #[test]
    fn fire_order_runs_aim_prepare_and_damage_pipeline() {
        let mut state = state();
        let red = spawn_tank(&mut state, TeamId::RED, Point::new(5, 5));
        let green = spawn_tank(&mut state, TeamId::GREEN, Point::new(8, 5));

        let mut data = state.order_data(red);
        data.fire_on = Point::new(8, 5);
        data.move_to = Point::new(8, 5);
        state.set_order_data(red, data);
        state.set_order(red, UnitOrder::Fire, UnitOrderState::Init);

        // AIM (2 поворота) + отдача (3 кадра) + выстрел — на 7-м тике.
        for _ in 0..7 {
            state.tick();
        }

        let target = state.unit(green).unwrap();
        assert_eq!(target.hits, 18, "TANK 16 - armor 10 = 6 урона");
        let shooter = state.unit(red).unwrap();
        assert_eq!(shooter.ammo, 13, "--ammo");
        assert_eq!(shooter.shots, 1, "--shots");
        assert_eq!(shooter.live_attack, 0, "EndAttack после выстрела");
        assert_eq!(target.pin_count, 0);
    }

    #[test]
    fn missile_prepare_fire_transfers_live_attack_to_particle() {
        let mut state = state();
        let launcher = state
            .spawn_unit(
                UnitType::MissileCrawler,
                TeamId::RED,
                Point::new(5, 5),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .unwrap();
        let mut data = state.order_data(launcher);
        data.fire_on = Point::new(9, 9);
        state.set_order_data(launcher, data);

        let mut unit = state.unit(launcher).unwrap().clone();
        unit.live_attack = 7;
        prepare_fire(&mut state, &mut unit);
        assert_eq!(
            unit.ammo,
            unit.base_values
                .get_attribute(crate::unit_values::ATTRIB_AMMO) as u8
                - 1
        );
        assert_eq!(unit.state, UnitOrderState::FireInProgress);
        assert_eq!(unit.live_attack, 0);

        // Снаряд — в списке частиц, с целью и родителем.
        let particle = state
            .unit_list(UnitListKind::Particle)
            .iter()
            .copied()
            .find(|id| {
                state
                    .unit(*id)
                    .map(|unit| unit.parent_unit == Some(launcher))
                    .unwrap_or(false)
            })
            .expect("particle");
        let particle_unit = state.unit(particle).unwrap();
        assert_eq!(particle_unit.live_attack, 7);
        assert_eq!(state.order_data(particle).fire_on, Point::new(9, 9));
    }

    #[test]
    fn stealth_action_uses_dice_roll_and_reveals_on_failure() {
        let mut state = state();
        let tower = state
            .spawn_unit(
                UnitType::GoldRefinery,
                TeamId::GREEN,
                Point::new(5, 5),
                STATIONARY | ELECTRONIC_UNIT | crate::enums::unit_flags::BUILDING,
            )
            .unwrap();
        {
            let unit = state.unit_mut(tower).unwrap();
            unit.orders = UnitOrder::PowerOn;
            unit.state = UnitOrderState::ExecutingOrder;
        }
        let commando = state
            .spawn_unit(
                UnitType::Infiltrator,
                TeamId::RED,
                Point::new(5, 6),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .unwrap();
        {
            let unit = state.unit_mut(commando).unwrap();
            unit.parent_unit = Some(tower);
            unit.orders = UnitOrder::AwaitDisableUnit;
            unit.state = UnitOrderState::ExecutingOrder;
            unit.stealth_dice_roll = 0;
        }

        let mut infiltrator = state.unit(commando).unwrap().clone();
        let chance = get_stealth_chance_percentage(
            &state,
            &infiltrator,
            state.unit(tower).unwrap(),
            infiltrator.orders,
        );
        assert!(chance > 0);
        let experience = infiltrator.experience;
        assert!(attempt_stealth_action(&mut state, &mut infiltrator));
        assert_eq!(infiltrator.experience, experience + 1);
        // `AttemptStealthAction` списывает `shots` (не `ammo`).
        assert_eq!(infiltrator.shots, 0);

        // Провал при большом броске: диверсант раскрыт.
        infiltrator.stealth_dice_roll = 100;
        assert!(!attempt_stealth_action(&mut state, &mut infiltrator));
        assert!(infiltrator.is_visible_to_team(TeamId::GREEN));
    }

    #[test]
    fn reaction_fire_starts_fire_order() {
        let mut state = state();
        state.play_mode = PlayMode::TurnBased;
        let red = spawn_tank(&mut state, TeamId::RED, Point::new(5, 5));
        let green = spawn_tank(&mut state, TeamId::GREEN, Point::new(7, 5));
        state.update_visibility_status(false);
        {
            let unit = state.unit_mut(red).unwrap();
            unit.orders = UnitOrder::Sentry;
            unit.state = UnitOrderState::ExecutingOrder;
        }
        {
            let unit = state.unit_mut(green).unwrap();
            unit.orders = UnitOrder::Sentry;
            unit.state = UnitOrderState::ExecutingOrder;
        }

        assert!(check_reaction(&mut state, red, green));
        let shooter = state.unit(red).unwrap();
        assert_eq!(shooter.orders, UnitOrder::Fire);
        assert_eq!(shooter.state, UnitOrderState::Init);
        assert_eq!(state.order_data(red).fire_on, Point::new(7, 5));
        assert!(state.units_active[0].is_none());

        // `AddToDelayedReactionList` + `AssessAttacks` запускают реакцию у
        // обороняющейся команды (зелёный танк в радиусе красного).
        state.unit_mut(red).unwrap().orders = UnitOrder::Sentry;
        state.unit_mut(green).unwrap().orders = UnitOrder::Sentry;
        state.add_to_delayed_reaction_list(red);
        let triggered = assess_attacks(&mut state);
        assert!(triggered);
        assert_eq!(state.unit(green).unwrap().orders, UnitOrder::Fire);
    }

    #[test]
    fn seeded_stealth_rolls_are_reproducible() {
        let mut first = state();
        let mut second = state();
        first.rng.set_seed(0xDEAD_BEEF);
        second.rng.set_seed(0xDEAD_BEEF);
        for _ in 0..64 {
            let a = roll_stealth_dice(&mut first);
            let b = roll_stealth_dice(&mut second);
            assert_eq!(a, b);
            assert!(a < 101);
        }
        assert_eq!(first.rng.state(), second.rng.state());
    }

    #[test]
    fn angle_helpers_match_cpp_tables() {
        assert_eq!(get_target_angle(1, 0), 2);
        assert_eq!(get_target_angle(0, 1), 4);
        assert_eq!(get_target_angle(-1, 0), 6);
        assert_eq!(get_target_angle(0, -1), 0);
        assert_eq!(get_target_angle(1, 1), 3);
        assert_eq!(get_target_angle(-1, -1), 7);
        assert_eq!(get_firing_angle(1, 0), 4);
        assert_eq!(get_firing_angle(0, 1), 8);
        assert_eq!(get_firing_angle(0, -1), 0);
        assert_eq!(get_firing_angle(1, 1), 6);
    }

    #[test]
    fn zone_walker_visits_circle_row_major() {
        let cells = zone_cells(Point::new(5, 5), 1);
        // Круг радиуса 1 — 3×3 без углов? C++ проверяет distance² <= range²,
        // поэтому углы (dx=1,dy=1 → 2 > 1) исключены: 5 клеток.
        assert_eq!(cells.len(), 5);
        assert!(cells.contains(&Point::new(5, 5)));
        assert!(!cells.contains(&Point::new(6, 6)));
        // Порядок: y возрастает, внутри строки x возрастает.
        let mut sorted = cells.clone();
        sorted.sort_by_key(|p| (p.y, p.x));
        assert_eq!(cells, sorted);
    }

    #[test]
    fn capture_and_disable_change_target_state() {
        let mut state = state();
        let tower = state
            .spawn_unit(
                UnitType::GoldRefinery,
                TeamId::GREEN,
                Point::new(5, 5),
                STATIONARY | ELECTRONIC_UNIT | crate::enums::unit_flags::BUILDING,
            )
            .unwrap();
        {
            let unit = state.unit_mut(tower).unwrap();
            unit.orders = UnitOrder::PowerOn;
            unit.state = UnitOrderState::ExecutingOrder;
        }
        let commando = state
            .spawn_unit(
                UnitType::Infiltrator,
                TeamId::RED,
                Point::new(5, 6),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .unwrap();

        capture_unit(&mut state, commando, tower);
        let captured = state.unit(tower).unwrap();
        assert_eq!(captured.team, TeamId::RED);
        assert!(captured.is_visible_to_team(TeamId::RED));
        assert_eq!(captured.parent_unit, None);

        // Отключение: цель получает ORDER_DISABLE на 1+ ход.
        let target = state
            .spawn_unit(
                UnitType::Tank,
                TeamId::GREEN,
                Point::new(8, 8),
                MOBILE_LAND_UNIT | SELECTABLE,
            )
            .unwrap();
        disable_unit(&mut state, commando, target);
        let disabled = state.unit(target).unwrap();
        assert_eq!(disabled.orders, UnitOrder::Disable);
        assert!(disabled.disabled_turns_remaining >= 1);
        assert_eq!(disabled.path, None);
    }

    #[test]
    fn damage_is_deterministic_between_runs() {
        fn run() -> (Vec<u8>, i32) {
            let mut state = state();
            let red = spawn_tank(&mut state, TeamId::RED, Point::new(5, 5));
            let green = spawn_tank(&mut state, TeamId::GREEN, Point::new(8, 5));
            let mut data = state.order_data(red);
            data.fire_on = Point::new(8, 5);
            state.set_order_data(red, data);
            state.set_order(red, UnitOrder::Fire, UnitOrderState::Init);
            for _ in 0..12 {
                state.tick();
            }
            (
                state.snapshot_bytes(),
                i32::from(state.unit(green).unwrap().hits),
            )
        }

        let first = run();
        let second = run();
        assert_eq!(first, second);
        assert!(first.1 < 24);
    }

    #[test]
    fn disable_turns_follows_cpp_formula() {
        let mut infiltrator = UnitInfo::new(UnitType::Infiltrator, TeamId::RED, UnitId(1));
        infiltrator.experience = 4;
        let mut target = UnitInfo::new(UnitType::Tank, TeamId::GREEN, UnitId(2));
        target.speed = target.base_values.get_attribute(ATTRIB_SPEED) as u16;
        // turns(TANK)=4, speed=6: ((6*(4+8)) - (4*6)) / (4*6) = (72-24)/24 = 2.
        assert_eq!(access::disable_turns(&infiltrator, &target), 2);
        assert!(target.base_values.get_attribute(ATTRIB_TURNS) > 0);
    }
}
