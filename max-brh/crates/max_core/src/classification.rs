//! Типобезопасный слой классификации юнитов — «вид» над битовыми флагами (Ф2).
//!
//! Слой **ничего не меняет**: `UnitInfo::flags` остаётся сериализуемым
//! состоянием (снапшот/CRC — правило 1.3 `rust-programming-rules.md`), а всё
//! ниже — производные значения, вычислимые из уже существующих данных:
//!
//! * [`UnitType`] — по таблице [`crate::access::UNIT_TYPE_PROPERTIES`]
//!   (`SC_UNITS`) и [`crate::unit_values::BASE_UNIT_VALUES`] (`SC_ATTRI`);
//! * [`UnitInfo`] — по флагам **экземпляра** (`unit.flags`): в runtime они
//!   могут расходиться с таблицей типа (`HOVERING`, `EXPLODING`, снятый
//!   `STATIONARY`), поэтому у экземпляра своя классификация.
//!
//! Источник таксономии — `max-brh-docs/object-classifier.md` («Ключевые
//! критерии», «Оси классификации юнитов»). Оси:
//!
//! | Ось | Значения | Rust |
//! |---|---|---|
//! | Боевая роль | боевой / небоевой | [`CombatRole`] |
//! | Форма | здание / мобильный / стационарный-прочий | [`Mobility`] |
//! | Среда мобильного | воздух / амфибия / земля / море | [`Environment`] |
//! | Производственный | производящий / непроизводящий | [`UnitType::is_producing`] |
//! | Груз | нет / ресурсный / юнитный | [`CargoKind`] |
//!
//! # Маркерные типажи
//!
//! [`Combat`], [`NonCombat`], [`Building`], [`Mobile`], [`Stationary`], [`Air`],
//! [`Amphibious`], [`Ground`], [`Sea`], [`Producing`], [`NonProducing`] —
//! пустые маркеры (правило 7: без префиксов `Is`/`Has`, без методов): это
//! словарь ограничений для обобщённого кода фазы Ф3. В `max_core` тип юнита —
//! значение [`UnitType`], а не отдельный тип, поэтому `impl Combat for
//! UnitType` утверждал бы «боевой» про все 93 типа сразу; рантайм-истина оси
//! живёт в перечислении, маркер — только имя признака.
//!
//! Источник C++: `enums.hpp` (`BUILDING`, `STATIONARY`, `MOBILE_*_UNIT`),
//! `units.cpp` (`Units::LoadResource` — флаги `SC_UNITS`), `attributes.cpp`
//! (`Attributes::LoadResource` — `SC_ATTRI`), `cargo.cpp` (`Cargo_GetNetProduction`),
//! `unit.hpp` (`Unit::CargoType`).

use crate::access::unit_type_flags;
use crate::enums::unit_flags::{
    BUILDING, MOBILE_AIR_UNIT, MOBILE_LAND_UNIT, MOBILE_SEA_UNIT, STATIONARY,
};
use crate::enums::UnitType;
use crate::unit::UnitInfo;
use crate::unit_values::base_unit_values;
use crate::victory::{cargo_type_of, CargoType};

// ---------------------------------------------------------------------------
// Маркерные типажи (пустые)
// ---------------------------------------------------------------------------

/// Маркер: боевой юнит (`attack > 0`, `ATTRIB_ATTACK`).
///
/// C++: `SC_ATTRI` (`Attributes::LoadResource`), ось «Combat/NonCombat»
/// `object-classifier.md`.
pub trait Combat {}

/// Маркер: небоевой юнит (`attack = 0`).
pub trait NonCombat {}

/// Маркер: здание — `BUILDING` (`0x10`, 2×2-структура) либо турель
/// (семантическое исключение владельца, [`TURRET_UNIT_TYPES`]).
pub trait Building {}

/// Маркер: мобильный юнит — `STATIONARY` (`0x200`) снят.
pub trait Mobile {}

/// Маркер: стационарный-прочий (`STATIONARY` без `BUILDING`, не турель).
pub trait Stationary {}

/// Маркер: воздушная среда мобильного юнита (`MOBILE_AIR_UNIT`, `0x40`).
pub trait Air {}

/// Маркер: амфибийная среда (`MOBILE_SEA_UNIT | MOBILE_LAND_UNIT`).
pub trait Amphibious {}

/// Маркер: наземная среда (`MOBILE_LAND_UNIT` без `MOBILE_SEA_UNIT`).
pub trait Ground {}

/// Маркер: морская среда (`MOBILE_SEA_UNIT` без `MOBILE_LAND_UNIT`).
pub trait Sea {}

/// Маркер: производящий тип — ровно 12 типов `Cargo_GetNetProduction`
/// ([`PRODUCING_UNIT_TYPES`]).
pub trait Producing {}

/// Маркер: непроизводящий тип (вне [`PRODUCING_UNIT_TYPES`]).
pub trait NonProducing {}

// ---------------------------------------------------------------------------
// Данные предметной области: списки исключений
// ---------------------------------------------------------------------------

/// Турели — **семантически здания**, хотя флаг `BUILDING` (`0x10`) не
/// установлен: по `SC_UNITS` это 1×1 `STATIONARY`. Решение владельца
/// (`object-classifier.md`, «Ключевые критерии»).
///
/// C++: `GUNTURRT`/`ARTYTRRT`/`ANTIAIR`/`ANTIMSSL` (`SC_UNITS`).
pub const TURRET_UNIT_TYPES: [UnitType; 4] = [
    UnitType::GunTurret,
    UnitType::Artillery,
    UnitType::AntiAir,
    UnitType::MissileLauncher,
];

/// Производящие типы — ровно те, что разбирает `Cargo_GetNetProduction`
/// (`cargo.cpp:70`, 12 типов): 5 заводов юнитов + 6 ресурсных зданий + шахта.
///
/// C++: `Cargo_GetNetProduction` (`cargo.cpp:70`); `object-classifier.md`,
/// «Производственные юниты».
pub const PRODUCING_UNIT_TYPES: [UnitType; 12] = [
    UnitType::Shipyard,
    UnitType::LightVehiclePlant,
    UnitType::HeavyVehiclePlant,
    UnitType::TrainingHall,
    UnitType::AirUnitsPlant,
    UnitType::GoldRefinery,
    UnitType::PowerStation,
    UnitType::PowerGenerator,
    UnitType::Habitat,
    UnitType::ResearchCentre,
    UnitType::EcoSphere,
    UnitType::MiningStation,
];

/// Турель ли тип (см. [`TURRET_UNIT_TYPES`]).
#[inline]
pub fn is_turret(unit_type: UnitType) -> bool {
    TURRET_UNIT_TYPES.contains(&unit_type)
}

// ---------------------------------------------------------------------------
// Боевая роль
// ---------------------------------------------------------------------------

/// Боевая роль юнита — ось `object-classifier.md` «Боевой / Небоевой».
///
/// C++: `attack` (`ATTRIB_ATTACK`) из `SC_ATTRI` (`Attributes::LoadResource`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CombatRole {
    /// Боевой: `attack > 0`.
    Combat,
    /// Небоевой: `attack = 0`.
    NonCombat,
}

impl CombatRole {
    /// Классифицирует тип юнита по базовому атрибуту `attack`.
    #[inline]
    pub fn classify(unit_type: UnitType) -> Self {
        Self::from_attack(base_unit_values(unit_type).attack)
    }

    /// Классифицирует экземпляр по **его** атрибутам (`base_values`:
    /// клановые модификаторы и апгрейды типа этой команды).
    #[inline]
    pub fn classify_unit(unit: &UnitInfo) -> Self {
        Self::from_attack(unit.base_values.attack)
    }

    /// `attack > 0` — боевой; `attack = 0` — небоевой.
    #[inline]
    pub fn from_attack(attack: u16) -> Self {
        if attack > 0 {
            Self::Combat
        } else {
            Self::NonCombat
        }
    }

    /// Боевой ли тип.
    #[inline]
    pub fn is_combat(self) -> bool {
        matches!(self, Self::Combat)
    }
}

// ---------------------------------------------------------------------------
// Форма
// ---------------------------------------------------------------------------

/// Форма юнита — ось `object-classifier.md` «Здание / Мобильный /
/// Стационарный-прочий».
///
/// C++: `UNIT_TYPE_PROPERTIES` (`SC_UNITS`, `Units::LoadResource`), флаги
/// `BUILDING` (`0x10`) и `STATIONARY` (`0x200`); турели — исключение
/// ([`TURRET_UNIT_TYPES`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mobility {
    /// Здание: `BUILDING` установлен (2×2-структура) либо тип — турель.
    Building,
    /// Мобильный: `STATIONARY` снят (в т.ч. снаряды — так их видит и C++).
    Mobile,
    /// Стационарный-прочий: `STATIONARY` без `BUILDING`, не турель.
    Stationary,
}

impl Mobility {
    /// Классифицирует тип по флагам `SC_UNITS` и списку турелей.
    #[inline]
    pub fn classify(unit_type: UnitType) -> Self {
        Self::classify_flags(unit_type, unit_type_flags(unit_type))
    }

    /// Классифицирует экземпляр по флагам экземпляра (`unit.flags`), форма
    /// типа — только ради списка турелей.
    #[inline]
    pub fn classify_unit(unit: &UnitInfo) -> Self {
        Self::classify_flags(unit.unit_type, unit.flags)
    }

    fn classify_flags(unit_type: UnitType, flags: u32) -> Self {
        if flags & BUILDING != 0 || is_turret(unit_type) {
            Self::Building
        } else if flags & STATIONARY != 0 {
            Self::Stationary
        } else {
            Self::Mobile
        }
    }
}

// ---------------------------------------------------------------------------
// Среда мобильного
// ---------------------------------------------------------------------------

/// Среда мобильного юнита — ось `object-classifier.md` «Воздушный /
/// Амфибийный / Наземный / Морской».
///
/// C++: `MOBILE_AIR_UNIT` (`0x40`), `MOBILE_SEA_UNIT` (`0x80`),
/// `MOBILE_LAND_UNIT` (`0x100`) из `SC_UNITS`. `HOVERING` (`0x10000`) — **не
/// среда**, а runtime-состояние полёта (`UnitInfo::Take`/`Land`), поэтому не
/// участвует в классификации.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Environment {
    /// Воздушная: `MOBILE_AIR_UNIT`.
    Air,
    /// Наземно-морская: `MOBILE_SEA_UNIT | MOBILE_LAND_UNIT`.
    Amphibious,
    /// Наземная: `MOBILE_LAND_UNIT` без `MOBILE_SEA_UNIT`.
    Ground,
    /// Морская: `MOBILE_SEA_UNIT` без `MOBILE_LAND_UNIT`.
    Sea,
}

impl Environment {
    /// Среда типа юнита; `None` — флагов среды нет (стационарные здания,
    /// а также снаряды/следы без `MOBILE_*`: `ROCKET`, `TORPEDO`, `ALNMISSL`,
    /// `ALNTBALL`, `ALNABALL`, `RKTSMOKE`, `TRPBUBLE`).
    #[inline]
    pub fn classify(unit_type: UnitType) -> Option<Self> {
        Self::classify_flags(unit_type_flags(unit_type))
    }

    /// Среда экземпляра по его флагам (`unit.flags`); `HOVERING` не влияет.
    #[inline]
    pub fn classify_unit(unit: &UnitInfo) -> Option<Self> {
        Self::classify_flags(unit.flags)
    }

    fn classify_flags(flags: u32) -> Option<Self> {
        let air = flags & MOBILE_AIR_UNIT != 0;
        let sea = flags & MOBILE_SEA_UNIT != 0;
        let land = flags & MOBILE_LAND_UNIT != 0;

        if air {
            Some(Self::Air)
        } else if sea && land {
            Some(Self::Amphibious)
        } else if land {
            Some(Self::Ground)
        } else if sea {
            Some(Self::Sea)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Груз
// ---------------------------------------------------------------------------

/// Вид груза юнита — ось `object-classifier.md` «Cargo (тип)».
///
/// C++: `Unit::CargoType` (`unit.hpp`), `ResourceManager_GetUnit(type)
/// .GetCargoType()`; взаимно исключающие группы `CARGO_TYPE_*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CargoKind {
    /// Груза нет (`CARGO_TYPE_NONE`).
    None,
    /// Ресурсный груз (`RAW`/`FUEL`/`GOLD`).
    Resource,
    /// Юнитный груз (`LAND`/`SEA`/`AIR`) — транспорт/ангар/док.
    Unit,
}

impl CargoKind {
    /// Вид груза типа юнита по [`cargo_type_of`].
    #[inline]
    pub fn classify(unit_type: UnitType) -> Self {
        Self::from_cargo_type(cargo_type_of(unit_type))
    }

    /// Вид груза экземпляра: груз — свойство **типа**, флаги экземпляра его не
    /// меняют.
    #[inline]
    pub fn classify_unit(unit: &UnitInfo) -> Self {
        Self::classify(unit.unit_type)
    }

    /// Группирует `CARGO_TYPE_*` в три вида.
    #[inline]
    pub fn from_cargo_type(cargo_type: CargoType) -> Self {
        match cargo_type {
            CargoType::None => Self::None,
            CargoType::Raw | CargoType::Fuel | CargoType::Gold => Self::Resource,
            CargoType::Land | CargoType::Sea | CargoType::Air => Self::Unit,
        }
    }
}

// ---------------------------------------------------------------------------
// Классификация типа юнита (per-type)
// ---------------------------------------------------------------------------

impl UnitType {
    /// Боевая роль типа ([`CombatRole::classify`]).
    #[inline]
    pub fn combat_role(self) -> CombatRole {
        CombatRole::classify(self)
    }

    /// Форма типа ([`Mobility::classify`]).
    #[inline]
    pub fn mobility(self) -> Mobility {
        Mobility::classify(self)
    }

    /// Среда типа ([`Environment::classify`]).
    #[inline]
    pub fn environment(self) -> Option<Environment> {
        Environment::classify(self)
    }

    /// Вид груза типа ([`CargoKind::classify`]).
    #[inline]
    pub fn cargo_kind(self) -> CargoKind {
        CargoKind::classify(self)
    }

    /// Производящий ли тип (`Cargo_GetNetProduction`, [`PRODUCING_UNIT_TYPES`]).
    #[inline]
    pub fn is_producing(self) -> bool {
        PRODUCING_UNIT_TYPES.contains(&self)
    }
}

// ---------------------------------------------------------------------------
// Классификация экземпляра (per-instance)
// ---------------------------------------------------------------------------

impl UnitInfo {
    /// Боевая роль экземпляра по его `base_values` ([`CombatRole::classify_unit`]).
    #[inline]
    pub fn combat_role(&self) -> CombatRole {
        CombatRole::classify_unit(self)
    }

    /// Форма экземпляра по его флагам ([`Mobility::classify_unit`]).
    #[inline]
    pub fn mobility(&self) -> Mobility {
        Mobility::classify_unit(self)
    }

    /// Среда экземпляра по его флагам ([`Environment::classify_unit`]).
    #[inline]
    pub fn environment(&self) -> Option<Environment> {
        Environment::classify_unit(self)
    }

    /// Вид груза экземпляра ([`CargoKind::classify_unit`]).
    #[inline]
    pub fn cargo_kind(&self) -> CargoKind {
        CargoKind::classify_unit(self)
    }

    /// Производящий ли экземпляр (свойство типа).
    #[inline]
    pub fn is_producing(&self) -> bool {
        self.unit_type.is_producing()
    }
}

// ---------------------------------------------------------------------------
// Тесты: исчерпывающая сверка 93 типов с прямыми проверками флагов/атрибутов
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::{unit_type_flags, UNIT_TYPE_PROPERTIES};
    use crate::enums::unit_flags::HOVERING;
    use crate::team::TeamId;
    use crate::unit::UnitId;

    /// Прямая проверка бита флага `SC_UNITS` — независимо от классификатора.
    fn has_flag(unit_type: UnitType, mask: u32) -> bool {
        UNIT_TYPE_PROPERTIES[unit_type as usize].flags & mask != 0
    }

    /// Прямая проверка атрибута `SC_ATTRI`.
    fn base_attack(unit_type: UnitType) -> u16 {
        base_unit_values(unit_type).attack
    }

    /// Ожидаемая форма по флагам и списку турелей (из `object-classifier.md`).
    fn expected_mobility(unit_type: UnitType) -> Mobility {
        let is_building = has_flag(unit_type, BUILDING)
            || matches!(
                unit_type,
                UnitType::GunTurret
                    | UnitType::Artillery
                    | UnitType::AntiAir
                    | UnitType::MissileLauncher
            );

        if is_building {
            Mobility::Building
        } else if has_flag(unit_type, STATIONARY) {
            Mobility::Stationary
        } else {
            Mobility::Mobile
        }
    }

    /// Ожидаемая среда по флагам (`MOBILE_*`).
    fn expected_environment(unit_type: UnitType) -> Option<Environment> {
        let air = has_flag(unit_type, MOBILE_AIR_UNIT);
        let sea = has_flag(unit_type, MOBILE_SEA_UNIT);
        let land = has_flag(unit_type, MOBILE_LAND_UNIT);

        if air {
            Some(Environment::Air)
        } else if sea && land {
            Some(Environment::Amphibious)
        } else if land {
            Some(Environment::Ground)
        } else if sea {
            Some(Environment::Sea)
        } else {
            None
        }
    }

    #[test]
    fn combat_role_matches_attack_for_all_93_types() {
        assert_eq!(UnitType::ALL.len(), 93);

        let mut combat = 0;
        for &unit_type in UnitType::ALL {
            let attack = base_attack(unit_type);
            let expected = if attack > 0 {
                CombatRole::Combat
            } else {
                CombatRole::NonCombat
            };

            assert_eq!(
                CombatRole::classify(unit_type),
                expected,
                "{unit_type:?}: attack = {attack}"
            );
            assert_eq!(unit_type.combat_role(), expected, "{unit_type:?}");
            assert_eq!(
                CombatRole::from_attack(attack),
                expected,
                "{unit_type:?}: from_attack"
            );
            assert_eq!(expected.is_combat(), attack > 0, "{unit_type:?}");

            if expected == CombatRole::Combat {
                combat += 1;
            }
        }

        // 25 боевых типов в `SC_ATTRI` (турели, мины, мобильная техника).
        assert_eq!(combat, 25);
    }

    #[test]
    fn mobility_matches_shape_rules_for_all_93_types() {
        let (mut building, mut stationary, mut mobile) = (0, 0, 0);

        for &unit_type in UnitType::ALL {
            let expected = expected_mobility(unit_type);

            assert_eq!(
                Mobility::classify(unit_type),
                expected,
                "{unit_type:?}: flags = {:#010X}",
                unit_type_flags(unit_type)
            );
            assert_eq!(unit_type.mobility(), expected, "{unit_type:?}");

            // Инвариант данных: `BUILDING ⟹ STATIONARY` (`object-classifier.md`).
            if has_flag(unit_type, BUILDING) {
                assert!(
                    has_flag(unit_type, STATIONARY),
                    "{unit_type:?}: BUILDING без STATIONARY"
                );
            }

            match expected {
                Mobility::Building => building += 1,
                Mobility::Stationary => stationary += 1,
                Mobility::Mobile => mobile += 1,
            }
        }

        assert_eq!((building, stationary, mobile), (26, 24, 43));

        // Турели: флаг `BUILDING` снят, поэтому зданиями их делает только
        // семантическое исключение (решение владельца).
        for &turret in &TURRET_UNIT_TYPES {
            assert!(!has_flag(turret, BUILDING), "{turret:?}");
            assert!(has_flag(turret, STATIONARY), "{turret:?}");
            assert_eq!(Mobility::classify(turret), Mobility::Building, "{turret:?}");
        }
        assert!(is_turret(UnitType::GunTurret));
        assert!(!is_turret(UnitType::LandMine));
    }

    #[test]
    fn environment_matches_mobile_flags_for_all_93_types() {
        let mut counts = [0usize; 5];

        for &unit_type in UnitType::ALL {
            let expected = expected_environment(unit_type);

            assert_eq!(
                Environment::classify(unit_type),
                expected,
                "{unit_type:?}: flags = {:#010X}",
                unit_type_flags(unit_type)
            );
            assert_eq!(unit_type.environment(), expected, "{unit_type:?}");

            let air = has_flag(unit_type, MOBILE_AIR_UNIT);
            if air {
                // Воздух не совмещается с наземно-морскими флагами — иначе
                // приоритет `Air` скрывал бы амфибию.
                assert!(
                    !has_flag(unit_type, MOBILE_SEA_UNIT | MOBILE_LAND_UNIT),
                    "{unit_type:?}: MOBILE_AIR_UNIT вместе с SEA/LAND"
                );
            }

            // Среда определена только у мобильных типов.
            if expected.is_some() {
                assert_eq!(
                    Mobility::classify(unit_type),
                    Mobility::Mobile,
                    "{unit_type:?}: среда у немобильного типа"
                );
            }

            counts[match expected {
                None => 0,
                Some(Environment::Air) => 1,
                Some(Environment::Amphibious) => 2,
                Some(Environment::Ground) => 3,
                Some(Environment::Sea) => 4,
            }] += 1;
        }

        // Air=5, Amphibious=5, Ground=17, Sea=9, без среды=57.
        assert_eq!(counts, [57, 5, 5, 17, 9]);

        // Снаряды и следы — мобильные по флагу, но среды у них нет.
        for projectile in [
            UnitType::Rocket,
            UnitType::Torpedo,
            UnitType::AlienMissile,
            UnitType::AlienTankBall,
            UnitType::AlienArtilleryBall,
            UnitType::RocketSmoke,
            UnitType::TorpedoBubble,
        ] {
            assert_eq!(Mobility::classify(projectile), Mobility::Mobile);
            assert_eq!(Environment::classify(projectile), None);
        }
    }

    #[test]
    fn producing_matches_cargo_get_net_production_for_all_93_types() {
        // Независимый перечень — ровно ветки `Cargo_GetNetProduction`
        // (`cargo.cpp:70`).
        let expected: [UnitType; 12] = [
            UnitType::Shipyard,
            UnitType::LightVehiclePlant,
            UnitType::HeavyVehiclePlant,
            UnitType::TrainingHall,
            UnitType::AirUnitsPlant,
            UnitType::GoldRefinery,
            UnitType::PowerStation,
            UnitType::PowerGenerator,
            UnitType::Habitat,
            UnitType::ResearchCentre,
            UnitType::EcoSphere,
            UnitType::MiningStation,
        ];

        let mut producing = 0;
        for &unit_type in UnitType::ALL {
            let is_producing = expected.contains(&unit_type);

            assert_eq!(unit_type.is_producing(), is_producing, "{unit_type:?}");
            assert_eq!(
                PRODUCING_UNIT_TYPES.contains(&unit_type),
                is_producing,
                "{unit_type:?}: PRODUCING_UNIT_TYPES"
            );

            if is_producing {
                producing += 1;
                // Производящие здания небоевые (`object-classifier.md`).
                assert_eq!(
                    CombatRole::classify(unit_type),
                    CombatRole::NonCombat,
                    "{unit_type:?}"
                );
            }
        }
        assert_eq!(producing, 12);
        assert_eq!(PRODUCING_UNIT_TYPES.len(), 12);

        // `BARRACKS`/`SUPRTPLT` не производящие: их нет в
        // `Cargo_GetNetProduction` (ремонт/alien-здание).
        assert!(!UnitType::Barracks.is_producing());
        assert!(!UnitType::SupportPlant.is_producing());
    }

    #[test]
    fn cargo_kind_matches_cargo_type_for_all_93_types() {
        // Независимые перечни по группам `CARGO_TYPE_*` (`unit.hpp`).
        let resource: [UnitType; 14] = [
            UnitType::MaterialStorage,
            UnitType::MiningStation,
            UnitType::Constructor,
            UnitType::MineLayer,
            UnitType::SupplyTruck,
            UnitType::Engineer,
            UnitType::Bulldozer,
            UnitType::RepairUnit,
            UnitType::SeaMineLayer,
            UnitType::CargoShip,
            UnitType::FuelStorage,
            UnitType::FuelTruck,
            UnitType::GoldVault,
            UnitType::GoldTruck,
        ];
        let transport: [UnitType; 8] = [
            UnitType::Barracks,
            UnitType::Depot,
            UnitType::ArmouredPersonnelCarrier,
            UnitType::SeaTransport,
            UnitType::AirTransport,
            UnitType::Hangar,
            UnitType::LandingPad,
            UnitType::Dock,
        ];

        let mut counts = [0usize; 3];
        for &unit_type in UnitType::ALL {
            let expected = if resource.contains(&unit_type) {
                CargoKind::Resource
            } else if transport.contains(&unit_type) {
                CargoKind::Unit
            } else {
                CargoKind::None
            };

            assert_eq!(
                CargoKind::classify(unit_type),
                expected,
                "{unit_type:?}: cargo_type = {:?}",
                cargo_type_of(unit_type)
            );
            assert_eq!(unit_type.cargo_kind(), expected, "{unit_type:?}");

            counts[match expected {
                CargoKind::None => 0,
                CargoKind::Resource => 1,
                CargoKind::Unit => 2,
            }] += 1;
        }

        // None=71, Resource=14, Unit=8.
        assert_eq!(counts, [71, 14, 8]);

        // Группировка `CARGO_TYPE_*` — все 7 значений C++.
        assert_eq!(CargoKind::from_cargo_type(CargoType::None), CargoKind::None);
        for cargo_type in [CargoType::Raw, CargoType::Fuel, CargoType::Gold] {
            assert_eq!(
                CargoKind::from_cargo_type(cargo_type),
                CargoKind::Resource,
                "{cargo_type:?}"
            );
        }
        for cargo_type in [CargoType::Land, CargoType::Sea, CargoType::Air] {
            assert_eq!(
                CargoKind::from_cargo_type(cargo_type),
                CargoKind::Unit,
                "{cargo_type:?}"
            );
        }
    }

    #[test]
    fn documented_control_cases() {
        // Scout — амфибийный боевой.
        assert_eq!(Mobility::classify(UnitType::Scout), Mobility::Mobile);
        assert_eq!(
            Environment::classify(UnitType::Scout),
            Some(Environment::Amphibious)
        );
        assert_eq!(CombatRole::classify(UnitType::Scout), CombatRole::Combat);

        // Awac — воздушный небоевой.
        assert_eq!(
            Environment::classify(UnitType::Awac),
            Some(Environment::Air)
        );
        assert_eq!(CombatRole::classify(UnitType::Awac), CombatRole::NonCombat);

        // GunTurret — здание по решению владельца, хотя `BUILDING` снят.
        assert!(!has_flag(UnitType::GunTurret, BUILDING));
        assert_eq!(Mobility::classify(UnitType::GunTurret), Mobility::Building);
        assert_eq!(
            CombatRole::classify(UnitType::GunTurret),
            CombatRole::Combat
        );

        // LandMine — стационарный боевой юнит (не здание).
        assert_eq!(Mobility::classify(UnitType::LandMine), Mobility::Stationary);
        assert_eq!(CombatRole::classify(UnitType::LandMine), CombatRole::Combat);

        // Производство: очиститель золота — да, казармы — нет.
        assert!(UnitType::GoldRefinery.is_producing());
        assert!(!UnitType::Barracks.is_producing());
    }

    #[test]
    fn instance_classification_matches_type_when_flags_are_copied() {
        for &unit_type in UnitType::ALL {
            let mut unit = UnitInfo::new(unit_type, TeamId::RED, UnitId::NO_ID);
            unit.flags = unit_type_flags(unit_type);

            assert_eq!(unit.combat_role(), CombatRole::classify(unit_type));
            assert_eq!(unit.mobility(), Mobility::classify(unit_type));
            assert_eq!(unit.environment(), Environment::classify(unit_type));
            assert_eq!(unit.cargo_kind(), CargoKind::classify(unit_type));
            assert_eq!(unit.is_producing(), unit_type.is_producing());
        }
    }

    #[test]
    fn instance_classification_follows_instance_flags() {
        // Наземный танк.
        let mut tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId::NO_ID);
        tank.flags = unit_type_flags(UnitType::Tank);
        assert_eq!(tank.mobility(), Mobility::Mobile);
        assert_eq!(tank.environment(), Some(Environment::Ground));

        // Runtime: воздушные флаги вместо наземных — экземпляр классифицируется
        // по своим флагам, таблица типа не участвует.
        tank.flags = (tank.flags & !(MOBILE_SEA_UNIT | MOBILE_LAND_UNIT)) | MOBILE_AIR_UNIT;
        assert_eq!(tank.mobility(), Mobility::Mobile);
        assert_eq!(tank.environment(), Some(Environment::Air));

        // `HOVERING` — состояние полёта, а не среда.
        tank.flags |= HOVERING;
        assert_eq!(tank.environment(), Some(Environment::Air));

        // У здания динамически сняты `BUILDING`/`STATIONARY` — экземпляр
        // «мобильный», хотя тип остаётся зданием.
        let mut barracks = UnitInfo::new(UnitType::Barracks, TeamId::RED, UnitId::NO_ID);
        barracks.flags = unit_type_flags(UnitType::Barracks);
        assert_eq!(barracks.mobility(), Mobility::Building);
        barracks.flags &= !(BUILDING | STATIONARY);
        assert_eq!(barracks.mobility(), Mobility::Mobile);
        assert_eq!(barracks.environment(), None);
        assert_eq!(Mobility::classify(UnitType::Barracks), Mobility::Building);

        // Боевая роль экземпляра — по его `base_values` (апгрейды/клан).
        let mut tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId::NO_ID);
        assert_eq!(tank.combat_role(), CombatRole::Combat);
        tank.base_values.attack = 0;
        assert_eq!(tank.combat_role(), CombatRole::NonCombat);
        assert_eq!(CombatRole::classify(UnitType::Tank), CombatRole::Combat);

        // Производство и груз — свойства типа, флаги экземпляра их не меняют.
        let mut refinery = UnitInfo::new(UnitType::GoldRefinery, TeamId::RED, UnitId::NO_ID);
        refinery.flags = 0;
        assert!(refinery.is_producing());
        assert_eq!(refinery.cargo_kind(), CargoKind::None);

        let mut barracks = UnitInfo::new(UnitType::Barracks, TeamId::RED, UnitId::NO_ID);
        barracks.flags = 0;
        assert_eq!(barracks.cargo_kind(), CargoKind::Unit);
    }
}
