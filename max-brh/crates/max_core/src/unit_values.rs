//! Атрибуты типов юнитов (`UnitValues`) и таблицы базовых значений 93 типов.
//!
//! Оракул: `max-cpp-port/src/unitvalues.hpp` / `unitvalues.cpp` (класс
//! `UnitValues`, индексы `ATTRIB_*`), `teamunits.cpp` (`TeamUnits::Init` —
//! заполнение из атрибутов), `attributes.cpp` (таблица `SC_ATTRI`),
//! `clans.cpp` + `resource_manager.cpp` (`ResourceManager_InitClanUnitValues` —
//! клановые модификаторы базовых значений, `TeamUnits_UpgradeOffsetFactor`).
//!
//! Раскладка полей — как в C++ (порядок объявления сохранён, `fuel` —
//! runtime-поле и в V70 не сериализуется, см. `unitvalues.cpp::FileLoad`).
//!
//! ## Откуда взята таблица [`BASE_UNIT_VALUES`]
//!
//! C++-порт не держит таблицу в коде: значения читаются из ресурса `SC_ATTRI`
//! (JSON, `assets/PATCHES.RES`, `Attributes::LoadResource`). Генератор
//! `max-brh/tools/gen_unit_values.py` извлекает и расшифровывает этот ресурс
//! (XOR-таблица `ResourceManager_GenericTable`) и печатает
//! `src/unit_values_tables.in`, который подключается сюда через [`include!`].
//! Клановые модификаторы (`CLAN_TRADEOFFS`) — из ресурса `SC_CLANS`
//! (`Clans::GetUnitTradeoff`). Достоверность таблицы проверяется тестом
//! [`tests::base_values_match_real_v70_save`] на реальном V70-сейве: базовые
//! значения команды в файле равны `BASE + клановые модификаторы`.

use serde::{Deserialize, Serialize};

use crate::enums::{TeamClan, UnitType, UNIT_END};

/// Количество игровых типов юнитов (`UNIT_END`).
pub const UNIT_COUNT: usize = UNIT_END as usize;

// ---------------------------------------------------------------------------
// ATTRIB_* — индексы атрибутов (unitvalues.hpp)
// ---------------------------------------------------------------------------

/// Атрибут «атака» (`ATTRIB_ATTACK` = `RESEARCH_TOPIC_ATTACK`).
pub const ATTRIB_ATTACK: u8 = 0;
/// Атрибут «выстрелы за атаку» (`ATTRIB_ROUNDS` = `RESEARCH_TOPIC_SHOTS`).
pub const ATTRIB_ROUNDS: u8 = 1;
/// Атрибут «дальность» (`ATTRIB_RANGE` = `RESEARCH_TOPIC_RANGE`).
pub const ATTRIB_RANGE: u8 = 2;
/// Атрибут «броня» (`ATTRIB_ARMOR` = `RESEARCH_TOPIC_ARMOR`).
pub const ATTRIB_ARMOR: u8 = 3;
/// Атрибут «прочность» (`ATTRIB_HITS` = `RESEARCH_TOPIC_HITS`).
pub const ATTRIB_HITS: u8 = 4;
/// Атрибут «скорость» (`ATTRIB_SPEED` = `RESEARCH_TOPIC_SPEED`).
pub const ATTRIB_SPEED: u8 = 5;
/// Атрибут «обзор» (`ATTRIB_SCAN` = `RESEARCH_TOPIC_SCAN`).
pub const ATTRIB_SCAN: u8 = 6;
/// Атрибут «ходов на постройку» (`ATTRIB_TURNS` = `RESEARCH_TOPIC_COST`).
pub const ATTRIB_TURNS: u8 = 7;
/// Атрибут «боезапас».
pub const ATTRIB_AMMO: u8 = 8;
/// Атрибут «стрельба на ходу».
pub const ATTRIB_MOVE_AND_FIRE: u8 = 9;
/// Атрибут «топливо» (не сериализуется, кланами не меняется).
pub const ATTRIB_FUEL: u8 = 10;
/// Атрибут «вместимость груза».
pub const ATTRIB_STORAGE: u8 = 11;
/// Атрибут «радиус поражения по площади».
pub const ATTRIB_ATTACK_RADIUS: u8 = 12;
/// Атрибут «бонус агента» (`ATTRIB_AGENT_ADJUST`, у кланов — `Experience`).
pub const ATTRIB_AGENT_ADJUST: u8 = 13;
/// Количество атрибутов (`ATTRIB_COUNT`).
pub const ATTRIB_COUNT: u8 = 14;

/// Имя атрибута по индексу `ATTRIB_*` (для отладочных дампов/снапшотов).
pub const ATTRIB_NAMES: [&str; ATTRIB_COUNT as usize] = [
    "attack",
    "rounds",
    "range",
    "armor",
    "hits",
    "speed",
    "scan",
    "turns",
    "ammo",
    "move_and_fire",
    "fuel",
    "storage",
    "attack_radius",
    "agent_adjust",
];

// ---------------------------------------------------------------------------
// UnitValues
// ---------------------------------------------------------------------------

/// Атрибуты типа юнита — C++ `class UnitValues : FileObject`
/// (`unitvalues.hpp`).
///
/// Порядок полей повторяет C++ (это же порядок сериализации V70). `fuel` —
/// runtime-поле: объявлено в C++, но в файл не пишется.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct UnitValues {
    /// Ходов на постройку.
    pub turns: u16,
    /// Прочность.
    pub hits: u16,
    /// Броня.
    pub armor: u16,
    /// Атака.
    pub attack: u16,
    /// Скорость (очки движения).
    pub speed: u16,
    /// Топливо (runtime, не сериализуется).
    pub fuel: u16,
    /// Дальность атаки.
    pub range: u16,
    /// Выстрелов за атаку.
    pub rounds: u16,
    /// Стрельба на ходу (`ATTRIB_MOVE_AND_FIRE`, u8 как в C++).
    pub move_and_fire: u8,
    /// Радиус обзора.
    pub scan: u16,
    /// Вместимость груза (у `COMMANDO`/`REGENERATING_UNIT` — опыт, см. `UnitInfo`).
    pub storage: u16,
    /// Боезапас.
    pub ammo: u16,
    /// Радиус поражения по площади.
    pub attack_radius: u16,
    /// Бонус агента (диверсанта).
    pub agent_adjust: u16,
    /// «Марка» (уровень апгрейда), старт = 1 (`UnitValues::UnitValues`).
    pub version: u16,
    /// Занят ли слот значений (`m_is_in_use`; в V70 — `units_built != 0`).
    pub in_use: bool,
}

impl Default for UnitValues {
    fn default() -> Self {
        Self::new()
    }
}

impl UnitValues {
    /// Конструктор C++: все поля 0, `version = 1`, `m_is_in_use = false`.
    pub const fn new() -> Self {
        Self {
            turns: 0,
            hits: 0,
            armor: 0,
            attack: 0,
            speed: 0,
            fuel: 0,
            range: 0,
            rounds: 0,
            move_and_fire: 0,
            scan: 0,
            storage: 0,
            ammo: 0,
            attack_radius: 0,
            agent_adjust: 0,
            version: 1,
            in_use: false,
        }
    }

    /// `UnitValues::GetAttribute`: значение атрибута по индексу `ATTRIB_*`.
    ///
    /// Как и C++, неверный индекс — ошибка вызывающего кода (в release
    /// возвращается 0, как ветка `default` оригинала).
    pub fn get_attribute(&self, attribute: u8) -> i32 {
        match attribute {
            ATTRIB_TURNS => i32::from(self.turns),
            ATTRIB_HITS => i32::from(self.hits),
            ATTRIB_ARMOR => i32::from(self.armor),
            ATTRIB_ATTACK => i32::from(self.attack),
            ATTRIB_MOVE_AND_FIRE => i32::from(self.move_and_fire),
            ATTRIB_SPEED => i32::from(self.speed),
            ATTRIB_FUEL => i32::from(self.fuel),
            ATTRIB_RANGE => i32::from(self.range),
            ATTRIB_ROUNDS => i32::from(self.rounds),
            ATTRIB_SCAN => i32::from(self.scan),
            ATTRIB_STORAGE => i32::from(self.storage),
            ATTRIB_AMMO => i32::from(self.ammo),
            ATTRIB_ATTACK_RADIUS => i32::from(self.attack_radius),
            ATTRIB_AGENT_ADJUST => i32::from(self.agent_adjust),
            _ => {
                debug_assert!(
                    false,
                    "UnitValues::get_attribute: invalid index {attribute}"
                );
                0
            }
        }
    }

    /// `UnitValues::SetAttribute`: присваивание с сужением как в C++
    /// (`uint16_t = int32_t` — младшие 16 бит).
    pub fn set_attribute(&mut self, attribute: u8, value: i32) {
        match attribute {
            ATTRIB_TURNS => self.turns = value as u16,
            ATTRIB_HITS => self.hits = value as u16,
            ATTRIB_ARMOR => self.armor = value as u16,
            ATTRIB_ATTACK => self.attack = value as u16,
            ATTRIB_MOVE_AND_FIRE => self.move_and_fire = value as u8,
            ATTRIB_SPEED => self.speed = value as u16,
            ATTRIB_FUEL => self.fuel = value as u16,
            ATTRIB_RANGE => self.range = value as u16,
            ATTRIB_ROUNDS => self.rounds = value as u16,
            ATTRIB_SCAN => self.scan = value as u16,
            ATTRIB_STORAGE => self.storage = value as u16,
            ATTRIB_AMMO => self.ammo = value as u16,
            ATTRIB_ATTACK_RADIUS => self.attack_radius = value as u16,
            ATTRIB_AGENT_ADJUST => self.agent_adjust = value as u16,
            _ => debug_assert!(
                false,
                "UnitValues::set_attribute: invalid index {attribute}"
            ),
        }
    }

    /// `UnitValues::AddAttribute`: `field += value` с заворачиванием в разрядность
    /// поля (C++ считает в `int`, затем сужает при записи).
    pub fn add_attribute(&mut self, attribute: u8, value: i32) {
        let current = self.get_attribute(attribute);
        self.set_attribute(attribute, current.wrapping_add(value));
    }

    /// `UnitValues::UpdateVersion`: увеличивает «марку» только у занятых слотов.
    pub fn update_version(&mut self) {
        if self.in_use {
            self.version = self.version.wrapping_add(1);
        }
    }

    /// `UnitValues::MarkAsNotInUse`.
    pub fn mark_as_not_in_use(&mut self) {
        self.in_use = false;
    }

    /// `UnitValues::MarkAsInUse`.
    pub fn mark_as_in_use(&mut self) {
        self.in_use = true;
    }

    /// `UnitValues::GetAgentSkill` — уровень агента (число апгрейдов
    /// `ATTRIB_AGENT_ADJUST` сверх базы; C++: `agent_adjust` текущих значений).
    pub fn agent_skill(&self) -> u16 {
        self.agent_adjust
    }
}

impl PartialEq for UnitValues {
    /// `UnitValues::operator==`: сравнивает все поля, **кроме** `fuel`
    /// (runtime-поле не участвует в сравнении C++).
    fn eq(&self, other: &Self) -> bool {
        self.turns == other.turns
            && self.hits == other.hits
            && self.armor == other.armor
            && self.attack == other.attack
            && self.speed == other.speed
            && self.range == other.range
            && self.rounds == other.rounds
            && self.move_and_fire == other.move_and_fire
            && self.scan == other.scan
            && self.storage == other.storage
            && self.ammo == other.ammo
            && self.attack_radius == other.attack_radius
            && self.version == other.version
            && self.in_use == other.in_use
            && self.agent_adjust == other.agent_adjust
    }
}

impl Eq for UnitValues {}

// ---------------------------------------------------------------------------
// Таблицы: базовые значения 93 типов и клановые модификаторы
// ---------------------------------------------------------------------------

include!("unit_values_tables.in");

/// Базовые значения типа юнита без клановых модификаторов
/// (`TeamUnits::Init` + `UnitValues()`: `version = 1`, `in_use = false`).
#[inline]
pub fn base_unit_values(unit_type: UnitType) -> UnitValues {
    BASE_UNIT_VALUES[unit_type as usize]
}

/// Базовые значения по «сырому» индексу типа (0..93); `None` вне диапазона.
#[inline]
pub fn base_unit_values_by_index(index: u16) -> Option<UnitValues> {
    BASE_UNIT_VALUES.get(index as usize).copied()
}

/// Клановый модификатор атрибута — C++ `Clans::GetUnitTradeoff`.
///
/// Возвращает 0, если для пары (клан, тип) модификатора нет (C++ в этом случае
/// не вызывает `AddAttribute`).
pub fn clan_tradeoff(clan: TeamClan, unit_type: UnitType, attribute: u8) -> i16 {
    let unit = unit_type as u16;
    let clan = clan as u8;
    let mut index = 0;
    while index < CLAN_TRADEOFFS.len() {
        let (c, u, a, delta) = CLAN_TRADEOFFS[index];
        if c == clan && u == unit && a == attribute {
            return delta;
        }
        index += 1;
    }
    0
}

/// Применяет все клановые модификаторы к набору атрибутов —
/// C++ `ResourceManager_SetClanUpgradesFromClans` (пропускает `ATTRIB_FUEL`).
pub fn apply_clan_tradeoffs(values: &mut UnitValues, clan: TeamClan, unit_type: UnitType) {
    let unit = unit_type as u16;
    let clan_index = clan as u8;
    for &(c, u, attribute, delta) in CLAN_TRADEOFFS.iter() {
        if c == clan_index && u == unit && attribute != ATTRIB_FUEL {
            values.add_attribute(attribute, i32::from(delta));
        }
    }
}

/// Базовые и текущие значения команды с учётом клана —
/// C++ `ResourceManager_InitClanUnitValues` (base == current после инициализации).
///
/// Для `TeamClan::Random` (0) C++ заменяет клан случайным; здесь возвращается
/// таблица без модификаторов (выбор клана — отдельный шаг setup).
pub fn team_base_values(clan: TeamClan) -> [UnitValues; UNIT_COUNT] {
    let mut values = BASE_UNIT_VALUES;
    if clan != TeamClan::Random {
        for (index, item) in values.iter_mut().enumerate() {
            let unit_type = UnitType::try_from(index as u16).expect("index < UNIT_END");
            apply_clan_tradeoffs(item, clan, unit_type);
        }
    }
    values
}

/// `TeamUnits_UpgradeOffsetFactor`: шаг атрибута при апгрейде, зависит от
/// базового значения (1/2/5/10).
///
/// Замечание: полная стоимость апгрейда (`TeamUnits_GetUpgradeCost`) здесь не
/// реализована — она требует `ResearchMenu_CalculateFactor` и
/// `UpgradeControl_CalculateCost` с `std::pow(..., 7.5)` (DESIGN риск 1:
/// float-формулы выносятся в build-time-таблицы в P4). Все целочисленные части
/// (`base/current` значения, шаг смещения) — здесь.
pub fn upgrade_offset_factor(base: &UnitValues, attribute: u8) -> i32 {
    let value = base.get_attribute(attribute);
    if value < 10 {
        1
    } else if value < 25 {
        2
    } else if value < 50 {
        5
    } else {
        10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Базовая таблица покрывает ровно 93 типа (`UNIT_END`).
    #[test]
    fn table_covers_all_unit_types() {
        assert_eq!(BASE_UNIT_VALUES.len(), UNIT_COUNT);
        assert_eq!(UNIT_COUNT, 93);
        assert_eq!(UnitType::COUNT, UNIT_COUNT);
        for (index, values) in BASE_UNIT_VALUES.iter().enumerate() {
            let expected = UnitType::try_from(index as u16).expect("valid unit type");
            assert_eq!(base_unit_values(expected), *values);
            // C++ `UnitValues()`: version = 1, m_is_in_use = false.
            assert_eq!(values.version, 1, "type {index}");
            assert!(!values.in_use, "type {index}");
            assert_eq!(values.fuel, 0, "type {index}");
        }
    }

    /// Выборочная сверка с таблицей `SC_ATTRI` (см. `gen_unit_values.py`).
    #[test]
    fn representative_base_values_match_cpp_table() {
        // COMMTWR / POWERSTN: 12 ходов, 56 hp, 8 брони, обзор 3.
        let commtower = base_unit_values(UnitType::CommTower);
        assert_eq!(
            (commtower.turns, commtower.hits, commtower.armor),
            (12, 56, 8)
        );
        assert_eq!(commtower.scan, 3);
        // POWGEN: 4 хода, 14 hp.
        let powgen = base_unit_values(UnitType::PowerGenerator);
        assert_eq!((powgen.turns, powgen.hits), (4, 14));
        // TANK (`TANK`, индекс 51): 4 хода, 24 hp, 10 брони, 16 атаки,
        // 6 скорости, 4 дальности, 2 выстрела, 4 обзора, 14 патронов.
        let tank = base_unit_values(UnitType::Tank);
        assert_eq!(
            (
                tank.turns,
                tank.hits,
                tank.armor,
                tank.attack,
                tank.speed,
                tank.range,
                tank.rounds,
                tank.scan,
                tank.ammo,
                tank.attack_radius,
            ),
            (4, 24, 10, 16, 6, 4, 2, 4, 14, 0)
        );
        // SCOUT (`SCOUT`, 50): 3 хода, 16 hp, 12 атаки, 12 скорости, обзор 9.
        let scout = base_unit_values(UnitType::Scout);
        assert_eq!(
            (
                scout.turns,
                scout.hits,
                scout.attack,
                scout.speed,
                scout.scan
            ),
            (3, 16, 12, 12, 9)
        );
        // GUNTURRT (34): стационарный, 4 хода, 24 hp, 12 брони, обзор 4.
        let turret = base_unit_values(UnitType::GunTurret);
        assert_eq!(
            (turret.turns, turret.hits, turret.armor, turret.speed),
            (4, 24, 12, 0)
        );
        // WALDO (`WALDO`, 92) — пасхалка без атрибутов.
        let waldo = base_unit_values(UnitType::DeadWaldo);
        assert_eq!(waldo.hits, 0);
        assert_eq!(waldo.turns, 0);
    }

    /// Атрибутный доступ: индексы `ATTRIB_*` и порядок полей как в C++.
    #[test]
    fn attribute_access_matches_cpp_indices() {
        let mut values = UnitValues::new();
        for attribute in 0..ATTRIB_COUNT {
            values.set_attribute(attribute, i32::from(attribute) + 1);
            assert_eq!(values.get_attribute(attribute), i32::from(attribute) + 1);
        }
        assert_eq!(values.attack, 1);
        assert_eq!(values.rounds, 2);
        assert_eq!(values.range, 3);
        assert_eq!(values.armor, 4);
        assert_eq!(values.hits, 5);
        assert_eq!(values.speed, 6);
        assert_eq!(values.scan, 7);
        assert_eq!(values.turns, 8);
        assert_eq!(values.ammo, 9);
        assert_eq!(values.move_and_fire, 10);
        assert_eq!(values.fuel, 11);
        assert_eq!(values.storage, 12);
        assert_eq!(values.attack_radius, 13);
        assert_eq!(values.agent_adjust, 14);

        // `AddAttribute` сужает в разрядность поля (C++: int -> uint16_t).
        values.set_attribute(ATTRIB_HITS, 65_535);
        values.add_attribute(ATTRIB_HITS, 1);
        assert_eq!(values.hits, 0);
        values.set_attribute(ATTRIB_MOVE_AND_FIRE, 255);
        values.add_attribute(ATTRIB_MOVE_AND_FIRE, 1);
        assert_eq!(values.move_and_fire, 0);
    }

    /// `operator==` игнорирует `fuel` (как C++).
    #[test]
    fn equality_ignores_fuel() {
        let mut left = UnitValues::new();
        let mut right = UnitValues::new();
        assert_eq!(left, right);
        right.fuel = 7;
        assert_eq!(left, right);
        left.hits = 1;
        assert_ne!(left, right);
    }

    /// `UpdateVersion` увеличивает версию только у занятого слота.
    #[test]
    fn version_updates_only_in_use() {
        let mut values = UnitValues::new();
        values.update_version();
        assert_eq!(values.version, 1);
        values.mark_as_in_use();
        values.update_version();
        assert_eq!(values.version, 2);
        values.mark_as_not_in_use();
        values.update_version();
        assert_eq!(values.version, 2);
    }

    /// Клановые модификаторы: `Clan C` (Von Griffin) даёт Scout +2 атаки/скорости
    /// и +2 обзора, Radar +6 обзора, Infiltrator -8 ходов постройки
    /// (`SC_CLANS`, `Clans::GetUnitTradeoff`).
    #[test]
    fn clan_tradeoffs_apply() {
        let scout = UnitType::Scout;
        assert_eq!(clan_tradeoff(TeamClan::VonGriffin, scout, ATTRIB_ATTACK), 2);
        assert_eq!(clan_tradeoff(TeamClan::VonGriffin, scout, ATTRIB_SPEED), 2);
        assert_eq!(clan_tradeoff(TeamClan::VonGriffin, scout, ATTRIB_SCAN), 2);
        assert_eq!(
            clan_tradeoff(TeamClan::VonGriffin, UnitType::Radar, ATTRIB_SCAN),
            6
        );
        assert_eq!(clan_tradeoff(TeamClan::Random, scout, ATTRIB_ATTACK), 0);
        // Другой клан того же юнита — другой модификатор.
        assert_eq!(
            clan_tradeoff(TeamClan::SevenKnights, scout, ATTRIB_ATTACK),
            0
        );

        let plain = base_unit_values(scout);
        let clan_values = team_base_values(TeamClan::VonGriffin);
        let modified = clan_values[scout as usize];
        assert_eq!(modified.attack, plain.attack + 2);
        assert_eq!(modified.speed, plain.speed + 2);
        assert_eq!(modified.scan, plain.scan + 2);
        // Остальные типы не меняются.
        assert_eq!(
            clan_values[UnitType::Tank as usize],
            base_unit_values(UnitType::Tank)
        );
        // `Random` — таблица без модификаторов.
        assert_eq!(team_base_values(TeamClan::Random), BASE_UNIT_VALUES);
    }

    /// `TeamUnits_UpgradeOffsetFactor` — границы шага апгрейда.
    #[test]
    fn upgrade_offset_factor_boundaries() {
        let mut values = UnitValues::new();
        for (base, expected) in [
            (0, 1),
            (9, 1),
            (10, 2),
            (24, 2),
            (25, 5),
            (49, 5),
            (50, 10),
            (999, 10),
        ] {
            values.set_attribute(ATTRIB_HITS, base);
            assert_eq!(
                upgrade_offset_factor(&values, ATTRIB_HITS),
                expected,
                "base {base}"
            );
        }
    }

    /// Индексы `ATTRIB_*` совпадают с `ResearchTopic` (первые 8).
    #[test]
    fn research_topics_match_attribute_order() {
        use crate::enums::ResearchTopic;
        assert_eq!(ATTRIB_ATTACK, ResearchTopic::Attack as u8);
        assert_eq!(ATTRIB_ROUNDS, ResearchTopic::Shots as u8);
        assert_eq!(ATTRIB_RANGE, ResearchTopic::Range as u8);
        assert_eq!(ATTRIB_ARMOR, ResearchTopic::Armor as u8);
        assert_eq!(ATTRIB_HITS, ResearchTopic::Hits as u8);
        assert_eq!(ATTRIB_SPEED, ResearchTopic::Speed as u8);
        assert_eq!(ATTRIB_SCAN, ResearchTopic::Scan as u8);
        assert_eq!(ATTRIB_TURNS, ResearchTopic::Cost as u8);
        assert_eq!(ATTRIB_COUNT as usize, ATTRIB_NAMES.len());
    }
}
