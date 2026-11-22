//! Research and purchased upgrades — C++ `researchmenu.cpp`,
//! `abstractupgrademenu.cpp`, `upgradecontrol.cpp`, `teamunits.cpp`.
//!
//! # Oracle and formulas
//!
//! * `ResearchMenu_UpdateResearchProgress` (`researchmenu.cpp:109`):
//!   `turns = (pow(base + 0.1, 7.5) * 256 / factor) - (pow(base, 7.5) * 256 / factor)`,
//!   where `base = level / 10.0 + 1.0`, `factor` — the topic setting
//!   (`attack_factor`=16, `shots_factor`=16, `range_factor`=8, `armor_factor`=32,
//!   `hits_factor`=32, `speed_factor`=16, `scan_factor`=8, `cost_factor`=8;
//!   `settings.cpp:70-77`).
//! * `UpgradeControl_CalculateCost` (`upgradecontrol.cpp:214`):
//!   `pow((current - factor) / base, 7.5) * UpgradeControl_Factors[id]`
//!   (`{4, 4, 8, 2, 2, 4, 8, 16, 2}`), for `ATTRIB_TURNS` — the inverse fraction
//!   `base / (current - factor)`; for `base == 0` — 0.
//! * `ResearchMenu_ApplyUpgrades` (`researchmenu.cpp:137`) — the increment
//!   `new - old` applied to the current values of all 93 types, where
//!   `old/new` are computed from the **base** attributes of the team and the level.
//! * `TeamUnits_UpgradeOffsetFactor` / `TeamUnits_GetUpgradeCost`
//!   (`teamunits.cpp:189/271`) — the 1/2/5/10 step and the transition cost.
//!
//! # Integer `pow(x, 7.5)`
//!
//! The simulation forbids `f32`/`f64` (DESIGN §6.4), and `std::pow` also
//! depends on libm. Therefore `x^7.5 = x^7 * sqrt(x)` is computed in Q48
//! fixed point (`1 << 48`), and `sqrt` by integer Newton iteration:
//! `(p/q)^7.5 = p^7 * sqrt(p*q) / q^8`. The result is truncated to `i32` as
//! `(int32_t)double` in C++; values outside `i32` yield `i32::MIN` (the behavior
//! of x86 `cvttsd2si`, see `tools/upgrade_vectors`). The reference vectors
//! (`crates/max_core/tests/data/upgrade_vectors.txt`) are generated from
//! `std::pow` and compared bit for bit in `tests/research.rs`.

use crate::enums::{ResearchTopic, UnitType, RESEARCH_TOPIC_COUNT, UNIT_END};
use crate::team::{ResearchTopicInfo, TeamId, TeamInfo, TeamKind, TeamUnits};
use crate::unit_values::{
    UnitValues, ATTRIB_AGENT_ADJUST, ATTRIB_AMMO, ATTRIB_ARMOR, ATTRIB_ATTACK,
    ATTRIB_ATTACK_RADIUS, ATTRIB_HITS, ATTRIB_MOVE_AND_FIRE, ATTRIB_RANGE, ATTRIB_ROUNDS,
    ATTRIB_SCAN, ATTRIB_SPEED, ATTRIB_STORAGE, ATTRIB_TURNS,
};

/// Upgrade cost factors — C++ `UpgradeControl_Factors`
/// (`upgradecontrol.cpp:29`), index `ATTRIB_*`.
pub const UPGRADE_CONTROL_FACTORS: [i32; 9] = [4, 4, 8, 2, 2, 4, 8, 16, 2];

/// Upper bound of the displayed cost (`UPGRADECONTROL_UPGRADE_COST_LIMIT`).
pub const UPGRADE_COST_LIMIT: i32 = 800;

/// How many attributes can be purchased (C++ `UPGRADE_CONTROL_COUNT` = 9).
pub const UPGRADE_CONTROL_COUNT: usize = 9;

/// Default research rate settings — `settings.cpp:70-77`
/// (`attack_factor`, `shots_factor`, …, `cost_factor`), `ResearchTopic` order.
pub const RESEARCH_TOPIC_FACTORS: [i32; RESEARCH_TOPIC_COUNT] = [16, 16, 8, 32, 32, 16, 8, 8];

/// Fixed point for `pow(x, 7.5)` (`2^48`).
const POW_SCALE: u128 = 1 << 48;

/// `UpgradeControl_Factors[id]`; `0` for an unknown id.
#[inline]
pub fn upgrade_control_factor(id: u8) -> i32 {
    UPGRADE_CONTROL_FACTORS.get(id as usize).copied().unwrap_or(0)
}

/// `TeamUnits_UpgradeOffsetFactor` (`teamunits.cpp:271`) — attribute step:
/// `<10 → 1`, `<25 → 2`, `<50 → 5`, otherwise `10`.
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

/// `UpgradeControl::Init` (`upgradecontrol.cpp:110`): `ATTRIB_TURNS` always
/// changes by 1, the rest — by the `UpgradeOffsetFactor` step.
#[inline]
pub fn upgrade_amount(id: u8, team_base_value: i32) -> u16 {
    if id == ATTRIB_TURNS {
        1
    } else {
        upgrade_amount_for_base(team_base_value)
    }
}

/// Integer `sqrt(n)` (Newton, `u128`).
fn isqrt_u128(n: u128) -> u128 {
    if n == 0 {
        return 0;
    }
    // Initial approximation strictly above the root: 2^(ceil(bits/2)).
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

/// `(p/q)^7.5` in Q48; `None` if the value is definitely greater than `i32`
/// (intermediate `u128` overflow).
fn pow_7_5_scaled(p: u128, q: u128) -> Option<u128> {
    debug_assert!(q > 0);

    let mut value = POW_SCALE;
    for _ in 0..7 {
        value = value.checked_mul(p)? / q;
    }

    // sqrt(p/q) in Q48: sqrt(p * S^2 / q).
    let root_arg = p.checked_mul(POW_SCALE)?.checked_mul(POW_SCALE)? / q;
    let root = isqrt_u128(root_arg);

    value.checked_mul(root).map(|product| product / POW_SCALE)
}

/// Conversion of a positive `u128` to `i32` like `(int32_t)(double)` on x86:
/// out of range — `i32::MIN`.
#[inline]
fn truncate_to_i32(value: u128) -> i32 {
    if value > i32::MAX as u128 {
        i32::MIN
    } else {
        value as i32
    }
}

/// `UpgradeControl_CalculateCost` (`upgradecontrol.cpp:214`) — with integers.
///
/// `id` — `ATTRIB_*`, `current`/`base` — attribute values, `factor` —
/// `ResearchMenu_CalculateFactor` (the research bonus).
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
            // C++: pow(inf/NaN) → (int32_t) → INT_MIN on x86.
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

/// `UpgradeControl::CalculateCost` (`upgradecontrol.cpp:58`) — the difference
/// of level costs at step `upgrade_amount`, minimum 1.
pub fn upgrade_cost_step(id: u8, current: u16, factor: u16, team_base: u16) -> i32 {
    let amount = upgrade_amount(id, i32::from(team_base));
    upgrade_cost_step_with(id, current, factor, team_base, amount)
}

/// The same with an explicit step (needed by tests and `TeamUnits_GetUpgradeCost`).
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

/// `ResearchMenu_UpdateResearchProgress` (`researchmenu.cpp:109`) without
/// `std::pow`: `turns_to_complete` for the current level and `factor`.
///
/// `factor <= 0` would divide by zero in C++ (inf → `INT_MIN`); in
/// the simulation settings are always positive (1..128), so we return
/// `i32::MAX` — "never", without panicking.
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

/// `ResearchMenu_CalculateFactor` (`researchmenu.cpp:258`): the research contribution
/// to the upgrade cost — `base_attr * level / 10` (0 without research).
pub fn research_factor(base_value: i32, level: u32) -> i32 {
    if level == 0 {
        return 0;
    }
    (base_value * level as i32) / 10
}

/// `ResearchMenu_ApplyUpgrades`: `(old, new)` attribute values when moving
/// to `level` — an exact copy of the `RESEARCH_TOPIC_COST`/other branches.
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

/// Attribute of a research topic (`RESEARCH_TOPIC_*` = `ATTRIB_*`).
#[inline]
pub fn research_attribute(topic: ResearchTopic) -> u8 {
    topic as u8
}

/// `ResearchMenu_ApplyUpgrades` (`researchmenu.cpp:137`): apply one
/// completed topic level to all 93 current values of the team.
///
/// C++ copies the current values, adds `new - old`, calls
/// `UpdateVersion()` (only for an occupied slot) and `MarkAsNotInUse()`,
/// then writes the object back.
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

/// Research completion event (for the log/HUD).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResearchEvent {
    /// Team.
    pub team: TeamId,
    /// Topic.
    pub topic: ResearchTopic,
    /// New topic level.
    pub level: u32,
}

/// `ResearchMenu_NewTurn` (`researchmenu.cpp:224`): deduct allocation from
/// `turns_to_complete`, and on completion — the level and the upgrade.
///
/// Before deduction, topics with `allocation > 0` and
/// `turns_to_complete == 0` are initialized (C++ does this in `GameManager_ManageEconomy`
/// via `ResearchMenu_UpdateResearchProgress(team, topic, 0)` at the start of the
/// team's turn; here the same idempotent initialization is used so that the research turn
/// does not depend on the call order).
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

/// `ResearchMenu_UpdateResearchProgress` for one topic (allocation may be
/// negative when the center is reassigned).
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

/// `TeamUnits_GetUpgradeCost` without the difficulty multiplier: the difference
/// of `current` and `current ± offset` costs, minimum 1.
///
/// Unlike [`upgrade_cost_step_with`] (UI control), there is no check of
/// `current > amount` here: C++ `TeamUnits_GetUpgradeCost` subtracts the step unconditionally
/// (`teamunits.cpp:237`), and the `uint16_t` formula parameter truncates the value.
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

/// `TeamUnits_GetUpgradeCost` (`teamunits.cpp:189`) — the cost of the next
/// purchased attribute upgrade, including research and AI difficulty.
///
/// `opponent` — the team's `Opponent` (the multiplier applies only to computers).
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
        // Topics 0..7 match ATTRIB 0..7; COST (7) is a separate topic for
        // `ATTRIB_TURNS` and does not enter the unit cost (C++ default: 0).
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

/// 13 serializable attributes of packets 10/20 in C++ order —
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

/// Values for packet 10 from `UnitValues` (C++ order).
pub fn upgrade_packet_values(values: &UnitValues) -> [u16; 13] {
    let mut result = [0u16; 13];
    for (index, attribute) in UPGRADE_PACKET_ATTRIBUTES.iter().enumerate() {
        result[index] = clamp_u16(values.get_attribute(*attribute));
    }
    result
}

// ---------------------------------------------------------------------------
// Cost in the upgrade menu (control state)
// ---------------------------------------------------------------------------

/// One purchased-upgrade control — C++ `UpgradeControl` without UI fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpgradeQuote {
    /// `ATTRIB_*`.
    pub attribute: u8,
    /// Base value of the type (formula denominator, `team_base_value`).
    pub team_base: u16,
    /// Current value of the type when the menu was opened (`control_base_value`).
    pub control_base: u16,
    /// Research contribution (`ResearchMenu_CalculateFactor`).
    pub research_factor: u16,
    /// Step (`UpgradeOffsetFactor`).
    pub amount: u16,
    /// Current (editable) value.
    pub actual: u16,
}

impl UpgradeQuote {
    /// Creates a control like `UpgradeControl::Init`.
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

    /// `false` if `Increase` is not allowed (as `UpdateControlState`: price < limit,
    /// enough gold; for hits/speed — no overflow).
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

    /// `false` if `Decrease` is not allowed (below the base value).
    pub fn can_decrease(&self) -> bool {
        self.actual > self.control_base
    }

    /// `UpgradeControl::Increase`: deducts gold, increases the value.
    pub fn increase(&mut self, gold: &mut u32) -> bool {
        if !self.can_increase(*gold) {
            return false;
        }
        let cost = self.cost();
        *gold = gold.saturating_sub(cost as u32);
        self.actual = self.actual.saturating_add(self.amount);
        true
    }

    /// `UpgradeControl::Decrease`: refunds gold, decreases the value.
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
