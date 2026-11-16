//! Исследования и покупные апгрейды — C++ `researchmenu.cpp`,
//! `abstractupgrademenu.cpp`, `upgradecontrol.cpp`, `teamunits.cpp`.
//!
//! # Оракул и формулы
//!
//! * `ResearchMenu_UpdateResearchProgress` (`researchmenu.cpp:109`):
//!   `turns = (pow(base + 0.1, 7.5) * 256 / factor) - (pow(base, 7.5) * 256 / factor)`,
//!   где `base = level / 10.0 + 1.0`, `factor` — настройка темы
//!   (`attack_factor`=16, `shots_factor`=16, `range_factor`=8, `armor_factor`=32,
//!   `hits_factor`=32, `speed_factor`=16, `scan_factor`=8, `cost_factor`=8;
//!   `settings.cpp:70-77`).
//! * `UpgradeControl_CalculateCost` (`upgradecontrol.cpp:214`):
//!   `pow((current - factor) / base, 7.5) * UpgradeControl_Factors[id]`
//!   (`{4, 4, 8, 2, 2, 4, 8, 16, 2}`), для `ATTRIB_TURNS` — обратная дробь
//!   `base / (current - factor)`; для `base == 0` — 0.
//! * `ResearchMenu_ApplyUpgrades` (`researchmenu.cpp:137`) — прибавка
//!   `new - old` к текущим значениям всех 93 типов, где
//!   `old/new` считаются от **базовых** атрибутов команды и уровня.
//! * `TeamUnits_UpgradeOffsetFactor` / `TeamUnits_GetUpgradeCost`
//!   (`teamunits.cpp:189/271`) — шаг 1/2/5/10 и стоимость перехода.
//!
//! # Целочисленный `pow(x, 7.5)`
//!
//! Симуляции запрещены `f32`/`f64` (DESIGN §6.4), а `std::pow` к тому же
//! зависит от libm. Поэтому `x^7.5 = x^7 * sqrt(x)` считается в Q48
//! фиксированной точке (`1 << 48`), а `sqrt` — целочисленным Ньютоном:
//! `(p/q)^7.5 = p^7 * sqrt(p*q) / q^8`. Результат обрезается к `i32` как
//! `(int32_t)double` в C++; значения вне `i32` дают `i32::MIN` (поведение
//! x86 `cvttsd2si`, см. `tools/upgrade_vectors`). Эталонные векторы
//! (`crates/max_core/tests/data/upgrade_vectors.txt`) сгенерированы из
//! `std::pow` и сверяются бит-в-бит в `tests/research.rs`.

use crate::enums::{ResearchTopic, UnitType, RESEARCH_TOPIC_COUNT, UNIT_END};
use crate::team::{ResearchTopicInfo, TeamId, TeamInfo, TeamKind, TeamUnits};
use crate::unit_values::{
    UnitValues, ATTRIB_AGENT_ADJUST, ATTRIB_AMMO, ATTRIB_ARMOR, ATTRIB_ATTACK,
    ATTRIB_ATTACK_RADIUS, ATTRIB_HITS, ATTRIB_MOVE_AND_FIRE, ATTRIB_RANGE, ATTRIB_ROUNDS,
    ATTRIB_SCAN, ATTRIB_SPEED, ATTRIB_STORAGE, ATTRIB_TURNS,
};

/// Множители стоимости апгрейда — C++ `UpgradeControl_Factors`
/// (`upgradecontrol.cpp:29`), индекс `ATTRIB_*`.
pub const UPGRADE_CONTROL_FACTORS: [i32; 9] = [4, 4, 8, 2, 2, 4, 8, 16, 2];

/// Верхняя граница показываемой стоимости (`UPGRADECONTROL_UPGRADE_COST_LIMIT`).
pub const UPGRADE_COST_LIMIT: i32 = 800;

/// Сколько атрибутов можно покупать (C++ `UPGRADE_CONTROL_COUNT` = 9).
pub const UPGRADE_CONTROL_COUNT: usize = 9;

/// Настройки скорости исследований по умолчанию — `settings.cpp:70-77`
/// (`attack_factor`, `shots_factor`, …, `cost_factor`), порядок `ResearchTopic`.
pub const RESEARCH_TOPIC_FACTORS: [i32; RESEARCH_TOPIC_COUNT] = [16, 16, 8, 32, 32, 16, 8, 8];

/// Фиксированная точка для `pow(x, 7.5)` (`2^48`).
const POW_SCALE: u128 = 1 << 48;

/// `UpgradeControl_Factors[id]`; `0` для неизвестного id.
#[inline]
pub fn upgrade_control_factor(id: u8) -> i32 {
    UPGRADE_CONTROL_FACTORS.get(id as usize).copied().unwrap_or(0)
}

/// `TeamUnits_UpgradeOffsetFactor` (`teamunits.cpp:271`) — шаг атрибута:
/// `<10 → 1`, `<25 → 2`, `<50 → 5`, иначе `10`.
#[inline]
pub fn upgrade_amount_for_base(base_value: i32) -> u16 {
    if base_value < 10 {
        1
    } else if base_value < 25 {
        2
    } else if base_value < 50 {
        5
    } else {
        10
    }
}

/// `UpgradeControl::Init` (`upgradecontrol.cpp:110`): `ATTRIB_TURNS` всегда
/// меняется на 1, остальные — шагом `UpgradeOffsetFactor`.
#[inline]
pub fn upgrade_amount(id: u8, team_base_value: i32) -> u16 {
    if id == ATTRIB_TURNS {
        1
    } else {
        upgrade_amount_for_base(team_base_value)
    }
}

/// Целочисленный `sqrt(n)` (Ньютон, `u128`).
fn isqrt_u128(n: u128) -> u128 {
    if n == 0 {
        return 0;
    }
    // Начальное приближение строго выше корня: 2^(ceil(bits/2)).
    let bits = 128 - n.leading_zeros();
    let mut x = 1u128 << ((bits + 1) / 2);
    loop {
        let y = (x + n / x) / 2;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// `(p/q)^7.5` в Q48; `None`, если значение заведомо больше `i32`
/// (промежуточное переполнение `u128`).
fn pow_7_5_scaled(p: u128, q: u128) -> Option<u128> {
    debug_assert!(q > 0);

    let mut value = POW_SCALE;
    for _ in 0..7 {
        value = value.checked_mul(p)? / q;
    }

    // sqrt(p/q) в Q48: sqrt(p * S^2 / q).
    let root_arg = p.checked_mul(POW_SCALE)?.checked_mul(POW_SCALE)? / q;
    let root = isqrt_u128(root_arg);

    value.checked_mul(root).map(|product| product / POW_SCALE)
}

/// Приведение положительного `u128` к `i32` как `(int32_t)(double)` на x86:
/// вне диапазона — `i32::MIN`.
#[inline]
fn truncate_to_i32(value: u128) -> i32 {
    if value > i32::MAX as u128 {
        i32::MIN
    } else {
        value as i32
    }
}

/// `UpgradeControl_CalculateCost` (`upgradecontrol.cpp:214`) — целочисленно.
///
/// `id` — `ATTRIB_*`, `current`/`base` — значения атрибута, `factor` —
/// `ResearchMenu_CalculateFactor` (прибавка от исследований).
pub fn upgrade_control_cost(id: u8, current: u16, factor: u16, base: u16) -> i32 {
    if id as usize >= UPGRADE_CONTROL_COUNT {
        return 0;
    }

    if id == ATTRIB_TURNS {
        if base == 0 {
            return 0;
        }
        let denominator = i32::from(current) - i32::from(factor);
        if denominator <= 0 {
            // C++: pow(inf/NaN) → (int32_t) → INT_MIN на x86.
            return i32::MIN;
        }
        let Some(value) = pow_7_5_scaled(u128::from(base), denominator as u128) else {
            return i32::MIN;
        };
        let scaled = value
            .checked_mul(UPGRADE_CONTROL_FACTORS[id as usize] as u128)
            .map(|product| product / POW_SCALE);
        match scaled {
            Some(result) => truncate_to_i32(result),
            None => i32::MIN,
        }
    } else if base > 0 {
        let numerator = i32::from(current) - i32::from(factor);
        if numerator <= 0 {
            return i32::MIN;
        }
        let Some(value) = pow_7_5_scaled(numerator as u128, u128::from(base)) else {
            return i32::MIN;
        };
        let scaled = value
            .checked_mul(UPGRADE_CONTROL_FACTORS[id as usize] as u128)
            .map(|product| product / POW_SCALE);
        match scaled {
            Some(result) => truncate_to_i32(result),
            None => i32::MIN,
        }
    } else {
        0
    }
}

/// `UpgradeControl::CalculateCost` (`upgradecontrol.cpp:58`) — разница
/// стоимостей уровня при шаге `upgrade_amount`, минимум 1.
pub fn upgrade_cost_step(id: u8, current: u16, factor: u16, team_base: u16) -> i32 {
    let amount = upgrade_amount(id, i32::from(team_base));
    upgrade_cost_step_with(id, current, factor, team_base, amount)
}

/// То же с явным шагом (нужно тестам и `TeamUnits_GetUpgradeCost`).
pub fn upgrade_cost_step_with(
    id: u8,
    current: u16,
    factor: u16,
    team_base: u16,
    amount: u16,
) -> i32 {
    let base_cost = upgrade_control_cost(id, current, factor, team_base);

    if id == ATTRIB_TURNS {
        if current > amount {
            let lower = current - amount;
            let cost = upgrade_control_cost(id, lower, factor, team_base).wrapping_sub(base_cost);
            cost.max(1)
        } else {
            0
        }
    } else if team_base > 0 {
        let higher = current.saturating_add(amount);
        let cost = upgrade_control_cost(id, higher, factor, team_base).wrapping_sub(base_cost);
        cost.max(1)
    } else {
        0
    }
}

/// `ResearchMenu_UpdateResearchProgress` (`researchmenu.cpp:109`) без
/// `std::pow`: `turns_to_complete` для текущего уровня и `factor`.
///
/// `factor <= 0` в C++ дал бы деление на ноль (inf → `INT_MIN`); в
/// симуляции настройки всегда положительны (1..128), поэтому возвращаем
/// `i32::MAX` — «никогда», без паники.
pub fn research_turns_to_complete(level: u32, factor: i32) -> i32 {
    if factor <= 0 {
        return i32::MAX;
    }

    let current = u128::from(level) + 10;
    let next = u128::from(level) + 11;
    let (Some(current_pow), Some(next_pow)) =
        (pow_7_5_scaled(current, 10), pow_7_5_scaled(next, 10))
    else {
        return i32::MIN;
    };

    let diff = next_pow.saturating_sub(current_pow);
    let Some(scaled) = diff.checked_mul(256) else {
        return i32::MIN;
    };
    let denominator = (factor as u128) * POW_SCALE;
    truncate_to_i32(scaled / denominator)
}

/// `ResearchMenu_CalculateFactor` (`researchmenu.cpp:258`): вклад исследований
/// в стоимость апгрейда — `base_attr * level / 10` (0 без исследований).
pub fn research_factor(base_value: i32, level: u32) -> i32 {
    if level == 0 {
        return 0;
    }
    (base_value * level as i32) / 10
}

/// `ResearchMenu_ApplyUpgrades`: `(old, new)` значения атрибута при переходе
/// на `level` — точная копия веток `RESEARCH_TOPIC_COST`/остальных.
pub fn research_attribute_values(topic: ResearchTopic, base_value: i32, level: u32) -> (i32, i32) {
    let level = level as i64;
    let value = i64::from(base_value);
    let (mut old_value, mut new_value) = if topic == ResearchTopic::Cost {
        let old = ((level + 9) / 2 + value * 10) / (level + 9);
        let new = ((level + 10) / 2 + value * 10) / (level + 10);
        (old, new)
    } else {
        (((level - 1) * value) / 10, (level * value) / 10)
    };

    if topic == ResearchTopic::Cost {
        old_value = old_value.max(1);
        new_value = new_value.max(1);
    }
    if topic == ResearchTopic::Hits || topic == ResearchTopic::Speed {
        old_value = old_value.min(i64::from(u16::MAX));
        new_value = new_value.min(i64::from(u16::MAX));
    }

    (old_value as i32, new_value as i32)
}

/// Атрибут темы исследования (`RESEARCH_TOPIC_*` = `ATTRIB_*`).
#[inline]
pub fn research_attribute(topic: ResearchTopic) -> u8 {
    topic as u8
}

/// `ResearchMenu_ApplyUpgrades` (`researchmenu.cpp:137`): применить один
/// завершённый уровень темы ко всем 93 текущим значениям команды.
///
/// C++ копирует текущие значения, прибавляет `new - old`, вызывает
/// `UpdateVersion()` (только для занятого слота) и `MarkAsNotInUse()`,
/// затем пишет объект обратно.
pub fn apply_research_upgrades(team_units: &mut TeamUnits, topic: ResearchTopic, level: u32) {
    let attribute = research_attribute(topic);

    for unit_type in 0..UNIT_END as usize {
        let base_value = team_units.base_values[unit_type].get_attribute(attribute);
        let (old_value, new_value) = research_attribute_values(topic, base_value, level);

        if new_value != old_value {
            let current = &mut team_units.current_values[unit_type];
            current.add_attribute(attribute, new_value.wrapping_sub(old_value));
            current.update_version();
            current.mark_as_not_in_use();
        }
    }
}

/// Событие завершения исследования (для журнала/HUD).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResearchEvent {
    /// Команда.
    pub team: TeamId,
    /// Тема.
    pub topic: ResearchTopic,
    /// Новый уровень темы.
    pub level: u32,
}

/// `ResearchMenu_NewTurn` (`researchmenu.cpp:224`): списать allocation с
/// `turns_to_complete`, при завершении — уровень и апгрейд.
///
/// Перед списанием инициализируются темы с `allocation > 0` и
/// `turns_to_complete == 0` (в C++ это делает `GameManager_ManageEconomy`
/// через `ResearchMenu_UpdateResearchProgress(team, topic, 0)` на старте хода
/// команды; здесь та же идемпотентная инициализация, чтобы ход исследований
/// не зависел от порядка вызовов).
pub fn research_new_turn(
    team_units: &mut TeamUnits,
    info: &mut TeamInfo,
    team: TeamId,
) -> Vec<ResearchEvent> {
    let mut events = Vec::new();

    for index in 0..RESEARCH_TOPIC_COUNT {
        let Some(topic) = ResearchTopic::try_from(index as u8).ok() else {
            continue;
        };

        if info.research_topics[index].allocation == 0 {
            continue;
        }

        if info.research_topics[index].turns_to_complete == 0 {
            let factor = RESEARCH_TOPIC_FACTORS[index];
            let turns = research_turns_to_complete(info.research_topics[index].research_level, factor);
            info.research_topics[index].turns_to_complete = turns.max(0) as u32;
        }

        let allocation = info.research_topics[index].allocation;
        info.research_topics[index].turns_to_complete =
            info.research_topics[index].turns_to_complete.saturating_sub(allocation);

        if info.research_topics[index].turns_to_complete == 0 {
            info.research_topics[index].research_level += 1;
            apply_research_upgrades(team_units, topic, info.research_topics[index].research_level);
            events.push(ResearchEvent {
                team,
                topic,
                level: info.research_topics[index].research_level,
            });
        }
    }

    events
}

/// `ResearchMenu_UpdateResearchProgress` для одной темы (allocation может быть
/// отрицательным при переназначении центра).
pub fn update_research_progress(topic: &mut ResearchTopicInfo, factor: i32, allocation: i32) {
    let turns = research_turns_to_complete(topic.research_level, factor);
    let new_allocation = i64::from(topic.allocation) + i64::from(allocation);
    topic.allocation = new_allocation.max(0) as u32;

    if topic.allocation != 0 && topic.turns_to_complete == 0 {
        topic.turns_to_complete = turns.max(0) as u32;
    }

    if topic.allocation == 0 && topic.turns_to_complete == turns.max(0) as u32 {
        topic.turns_to_complete = 0;
    }
}

/// `TeamUnits_GetUpgradeCost` без множителя сложности: разница стоимостей
/// `current` и `current ± offset`, минимум 1.
///
/// В отличие от [`upgrade_cost_step_with`] (UI-контрол), здесь нет проверки
/// `current > amount`: C++ `TeamUnits_GetUpgradeCost` вычитает шаг безусловно
/// (`teamunits.cpp:237`), а `uint16_t`-параметр формулы усекает значение.
pub fn team_upgrade_cost_step(
    id: u8,
    current: u16,
    factor: u16,
    base: u16,
    offset: u16,
) -> i32 {
    let first = upgrade_control_cost(id, current, factor, base);
    let next = if id == ATTRIB_TURNS {
        i32::from(current) - i32::from(offset)
    } else {
        i32::from(current) + i32::from(offset)
    };
    let next = (next as u32 & 0xFFFF) as u16;
    let second = upgrade_control_cost(id, next, factor, base);
    second.wrapping_sub(first).max(1)
}

/// `TeamUnits_GetUpgradeCost` (`teamunits.cpp:189`) — стоимость следующего
/// покупного апгрейда атрибута с учётом исследований и сложности ИИ.
///
/// `opponent` — `Opponent` команды (множитель только для компьютеров).
pub fn team_upgrade_cost(
    team_units: &TeamUnits,
    info: &TeamInfo,
    unit_type: UnitType,
    attribute: u8,
    opponent: u8,
) -> i32 {
    let unit_index = unit_type as usize;
    let base_values = &team_units.base_values[unit_index];
    let current_values = &team_units.current_values[unit_index];

    let factor = if attribute == ATTRIB_TURNS || attribute as usize >= RESEARCH_TOPIC_COUNT {
        0
    } else {
        // Темы 0..7 совпадают с ATTRIB 0..7; COST (7) — отдельная тема для
        // `ATTRIB_TURNS` и в стоимость юнита не входит (C++ default: 0).
        let topic = ResearchTopic::try_from(attribute).ok();
        match topic {
            Some(topic) => {
                let level = info.research_topics[topic as usize].research_level;
                research_factor(base_values.get_attribute(attribute), level)
            }
            None => 0,
        }
    };

    let base_value = base_values.get_attribute(attribute);
    let current_value = current_values.get_attribute(attribute);
    let offset = upgrade_amount_for_base(base_value);

    let mut cost = team_upgrade_cost_step(
        attribute,
        current_value as u16,
        factor as u16,
        base_value as u16,
        offset,
    );

    if info.team_type == TeamKind::Computer {
        cost = match crate::enums::Opponent::try_from(opponent).ok() {
            Some(crate::enums::Opponent::Master) => (cost * 4) / 5,
            Some(crate::enums::Opponent::God) => (cost * 2) / 3,
            Some(crate::enums::Opponent::Clueless) => (cost * 5) / 4,
            _ => cost,
        };
    }

    cost.max(1)
}

#[inline]
fn clamp_u16(value: i32) -> u16 {
    value.clamp(0, i32::from(u16::MAX)) as u16
}

/// 13 сериализуемых атрибутов пакета 10/20 в порядке C++ —
/// `turns, hits, armor, attack, speed, range, rounds, move_and_fire, scan,
/// storage, ammo, attack_radius, agent_adjust`.
pub const UPGRADE_PACKET_ATTRIBUTES: [u8; 13] = [
    ATTRIB_TURNS,
    ATTRIB_HITS,
    ATTRIB_ARMOR,
    ATTRIB_ATTACK,
    ATTRIB_SPEED,
    ATTRIB_RANGE,
    ATTRIB_ROUNDS,
    ATTRIB_MOVE_AND_FIRE,
    ATTRIB_SCAN,
    ATTRIB_STORAGE,
    ATTRIB_AMMO,
    ATTRIB_ATTACK_RADIUS,
    ATTRIB_AGENT_ADJUST,
];

/// Значения для пакета 10 из `UnitValues` (порядок C++).
pub fn upgrade_packet_values(values: &UnitValues) -> [u16; 13] {
    let mut result = [0u16; 13];
    for (index, attribute) in UPGRADE_PACKET_ATTRIBUTES.iter().enumerate() {
        result[index] = clamp_u16(values.get_attribute(*attribute));
    }
    result
}

// ---------------------------------------------------------------------------
// Стоимость в меню апгрейдов (состояние контрола)
// ---------------------------------------------------------------------------

/// Один контрол покупного апгрейда — C++ `UpgradeControl` без UI-полей.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpgradeQuote {
    /// `ATTRIB_*`.
    pub attribute: u8,
    /// Базовое значение типа (знаменатель формулы, `team_base_value`).
    pub team_base: u16,
    /// Текущее значение типа на момент открытия меню (`control_base_value`).
    pub control_base: u16,
    /// Вклад исследований (`ResearchMenu_CalculateFactor`).
    pub research_factor: u16,
    /// Шаг (`UpgradeOffsetFactor`).
    pub amount: u16,
    /// Текущее (редактируемое) значение.
    pub actual: u16,
}

impl UpgradeQuote {
    /// Создаёт контрол как `UpgradeControl::Init`.
    pub fn new(attribute: u8, team_base: u16, control_base: u16, research_factor: u16) -> Self {
        Self {
            attribute,
            team_base,
            control_base,
            research_factor,
            amount: upgrade_amount(attribute, i32::from(team_base)),
            actual: control_base,
        }
    }

    /// `UpgradeControl::CalculateCost`.
    pub fn cost(&self) -> i32 {
        upgrade_cost_step_with(
            self.attribute,
            self.actual,
            self.research_factor,
            self.team_base,
            self.amount,
        )
    }

    /// `false`, если `Increase` нельзя (как `UpdateControlState`: цена < лимита,
    /// золота хватает; для hits/speed — не переполнение).
    pub fn can_increase(&self, gold: u32) -> bool {
        if self.team_base == 0 {
            return false;
        }
        let cost = self.cost();
        if cost < 0 || cost >= UPGRADE_COST_LIMIT || u32::try_from(cost).unwrap_or(u32::MAX) > gold {
            return false;
        }
        if (self.attribute == ATTRIB_HITS || self.attribute == ATTRIB_SPEED)
            && self.actual == u16::MAX
        {
            return false;
        }
        true
    }

    /// `false`, если `Decrease` нельзя (ниже базового значения).
    pub fn can_decrease(&self) -> bool {
        self.actual > self.control_base
    }

    /// `UpgradeControl::Increase`: списывает золото, увеличивает значение.
    pub fn increase(&mut self, gold: &mut u32) -> bool {
        if !self.can_increase(*gold) {
            return false;
        }
        let cost = self.cost();
        *gold = gold.saturating_sub(cost as u32);
        self.actual = self.actual.saturating_add(self.amount);
        true
    }

    /// `UpgradeControl::Decrease`: возвращает золото, уменьшает значение.
    pub fn decrease(&mut self, gold: &mut u32) -> bool {
        if !self.can_decrease() {
            return false;
        }
        self.actual = self.actual.saturating_sub(self.amount);
        *gold = gold.saturating_add(self.cost().max(1) as u32);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_amount_matches_cpp_thresholds() {
        assert_eq!(upgrade_amount_for_base(0), 1);
        assert_eq!(upgrade_amount_for_base(9), 1);
        assert_eq!(upgrade_amount_for_base(10), 2);
        assert_eq!(upgrade_amount_for_base(24), 2);
        assert_eq!(upgrade_amount_for_base(25), 5);
        assert_eq!(upgrade_amount_for_base(49), 5);
        assert_eq!(upgrade_amount_for_base(50), 10);
        assert_eq!(upgrade_amount(ATTRIB_TURNS, 100), 1);
    }

    #[test]
    fn research_factor_matches_cpp() {
        assert_eq!(research_factor(20, 0), 0);
        assert_eq!(research_factor(20, 1), 2);
        assert_eq!(research_factor(20, 9), 18);
        assert_eq!(research_factor(7, 4), 2);
        assert_eq!(research_factor(50, 3), 15);
    }
}
