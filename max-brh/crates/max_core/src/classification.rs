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
//! критерии», «Оси классификации юнитов», «Не-юниты»).
//!
//! # Отделение не-юнитов (Ф4)
//!
//! [`ObjectKind`] отделяет **юниты** от **не-юнитов** (эффекты, декорации,
//! сюжетные объекты) на уровне семантики. Сам [`UnitType`] при этом **не
//! меняется**: 93 варианта с дискриминантами `0x00–0x5C` остаются `ResourceID`
//! (`SC_UNITS`) — от них зависят CRC, replay-дайджест и формат V70. `ObjectKind`
//! — производное значение, как [`Footprint`].
//!
//! Оси:
//!
//! | Ось | Значения | Rust |
//! |---|---|---|
//! | Вид объекта | юнит / эффект / декорация / сюжетная декорация | [`ObjectKind`] |
//! | Боевая роль | боевой / небоевой | [`CombatRole`] |
//! | Форма | здание / мобильный / стационарный-прочий | [`Mobility`] |
//! | Футпринт (размер на карте) | 1×1 / 2×2 | [`Footprint`] |
//! | Среда мобильного | воздух / амфибия / земля / море | [`Environment`] |
//! | Производственный | производящий / непроизводящий | [`UnitType::is_producing`] |
//! | Груз | нет / ресурсный / юнитный | [`CargoKind`] |
//!
//! # Решение по маркерным типажам (вариант C)
//!
//! Маркерные типажи без реализаций **не используются** (правило 1, решение
//! владельца): `UnitType` — закрытый `enum` из 93 вариантов, и `impl Combat for
//! UnitType` утверждал бы «боевой» про все 93 сразу — мёртвый код. Классификацию
//! несёт `enum` (единственный источник истины); поведение — поведенческие
//! типажи (`CanMove`, `CanAttack`) в обобщённом коде, а не маркеры категорий.
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
// Данные предметной области: не-юниты (Ф4)
// ---------------------------------------------------------------------------

/// Эффекты — **не-юниты**: снаряды, взрывы, обломки, следы (14 типов).
///
/// Источник — `object-classifier.md`, «Не-юниты» → «Эффект»:
/// `ROCKET`, `TORPEDO`, `ALNMISSL`, `ALNTBALL`, `ALNABALL` (снаряды);
/// `LNDEXPLD`, `AIREXPLD`, `SEAEXPLD`, `BLDEXPLD`, `HITEXPLD` (взрывы);
/// `LRGRUBLE`, `SMLRUBLE` (обломки); `RKTSMOKE`, `TRPBUBLE` (следы).
///
/// C++: отдельного признака нет — все они `ResourceID` из `SC_UNITS`;
/// отделение семантическое (Ф4), `UnitType` не меняется.
pub const EFFECT_UNIT_TYPES: [UnitType; 14] = [
    UnitType::Rocket,
    UnitType::Torpedo,
    UnitType::AlienMissile,
    UnitType::AlienTankBall,
    UnitType::AlienArtilleryBall,
    UnitType::LandExplosion,
    UnitType::AirExplosion,
    UnitType::SeaExplosion,
    UnitType::BuildingExplosion,
    UnitType::HitExplosion,
    UnitType::LargeRubble,
    UnitType::SmallRubble,
    UnitType::RocketSmoke,
    UnitType::TorpedoBubble,
];

/// Декорации/стройплощадки — **не-юниты** (6 типов): `LRGTAPE`, `SMLTAPE`,
/// `LRGSLAB`, `SMLSLAB`, `LRGCONES`, `SMLCONES`.
///
/// Источник — `object-classifier.md`, «Не-юниты» → «Декорация/стройплощадка».
pub const DECORATION_UNIT_TYPES: [UnitType; 6] = [
    UnitType::LargeTape,
    UnitType::SmallTape,
    UnitType::LargeSlab,
    UnitType::SmallSlab,
    UnitType::LargeCones,
    UnitType::SmallCones,
];

/// Сюжетные декорации (удалённые/скрытые) — **не-юниты** (3 типа): `MASTER`,
/// `HARVSTER`, `WALDO`.
///
/// Источник — `object-classifier.md`, «Не-юниты» → «Сюжетная декорация».
pub const PLOT_PROP_UNIT_TYPES: [UnitType; 3] = [
    UnitType::MasterBuilder,
    UnitType::Harvester,
    UnitType::DeadWaldo,
];

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
// Футпринт
// ---------------------------------------------------------------------------

/// Футпринт (размер на карте) — ось `object-classifier.md` «Футпринт».
///
/// **Лайаут, а не семантика:** ось читает ровно бит `BUILDING` (`0x10`,
/// «2×2-структура»). Турели — [`Mobility::Building`] семантически, но у них
/// флаг снят, поэтому [`Footprint::Single`]; инвариант
/// [`Footprint::Quad`] `⟹` [`Mobility::Building`] (обратное неверно).
///
/// Стабильные теги сериализации — **явные** дискриминанты (порядок вариантов
/// контрактом не является): `Single = 0`, `Quad = 1`.
///
/// C++: `BUILDING` (`enums.hpp`); `Hash_MapHash::Add/Remove` (`hash.cpp:188`,
/// `hash.cpp:227`); габарит `(flags & BUILDING) ? 2 : 1` (`units_manager.cpp:2056`,
/// `unitinfo.cpp:4086`, `task.cpp:314`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Footprint {
    /// 1×1 — одна клетка карты (мобильные, турели, 1×1-структуры, эффекты).
    Single = 0,
    /// 2×2 — четыре клетки карты (флаг `BUILDING`).
    Quad = 1,
}

impl Footprint {
    /// Футпринт типа юнита по флагам `SC_UNITS`.
    #[inline]
    pub fn classify(unit_type: UnitType) -> Self {
        Self::classify_flags(unit_type, unit_type_flags(unit_type))
    }

    /// Футпринт экземпляра по **его** флагам (`unit.flags`): если у экземпляра
    /// динамически снят `BUILDING`, он занимает 1×1.
    #[inline]
    pub fn classify_unit(unit: &UnitInfo) -> Self {
        Self::classify_flags(unit.unit_type, unit.flags)
    }

    /// Футпринт по флагу `BUILDING` (`0x10`): установлен — 2×2, снят — 1×1.
    ///
    /// Инвариант `Quad ⟹ Mobility::Building` проверяется `debug_assert!`: флаг
    /// `BUILDING` всегда даёт [`Mobility::Building`], обратное неверно (турель —
    /// `Building` + `Single`).
    #[inline]
    pub fn classify_flags(unit_type: UnitType, flags: u32) -> Self {
        let footprint = Self::from_flags(flags);
        debug_assert!(
            !footprint.is_quad()
                || Mobility::classify_flags(unit_type, flags) == Mobility::Building,
            "{unit_type:?}: Footprint::Quad при Mobility ≠ Building (flags = {flags:#010X})"
        );
        footprint
    }

    /// Футпринт по сырым флагам, когда тип неизвестен (лайаутные API: `MapHash`,
    /// `Access_*` принимают `flags`). Единственное место оси, читающее бит.
    #[inline]
    pub fn from_flags(flags: u32) -> Self {
        if flags & BUILDING != 0 {
            Self::Quad
        } else {
            Self::Single
        }
    }

    /// 2×2 ли футпринт.
    #[inline]
    pub fn is_quad(self) -> bool {
        matches!(self, Self::Quad)
    }

    /// 1×1 ли футпринт.
    #[inline]
    pub fn is_single(self) -> bool {
        matches!(self, Self::Single)
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
// Вид объекта: юнит / не-юнит (Ф4)
// ---------------------------------------------------------------------------

/// Вид объекта — семантическое отделение **юнитов** от **не-юнитов** (Ф4).
///
/// Это **производная** ось, как [`Footprint`]: сам [`UnitType`] остаётся
/// `ResourceID`-совместимым (`#[repr(u16)]`, 93 варианта, `0x00–0x5C`), потому
/// что его дискриминанты индексируют таблицы `SC_UNITS`/`SC_ATTRI`, входят в
/// CRC/replay-дайджест и в формат V70. Переклассификация — **без изменения
/// поведения**: эффекты и декорации остаются объектами тех же `ResourceID` в
/// мире, но больше не считаются «юнитами» в логическом классификаторе.
///
/// Стабильные теги сериализации — **явные** дискриминанты (порядок вариантов
/// контрактом не является): `Unit = 0`, `Effect = 1`, `Decoration = 2`,
/// `PlotProp = 3`.
///
/// Источник: `object-classifier.md`, разделы «Юниты» и «Не-юниты».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ObjectKind {
    /// Юнит: реальный юнит или здание (70 типов, `COMMTWR`..`ALNPLANE`).
    Unit = 0,
    /// Эффект: снаряд, взрыв, обломок, след ([`EFFECT_UNIT_TYPES`], 14).
    Effect = 1,
    /// Декорация/стройплощадка ([`DECORATION_UNIT_TYPES`], 6).
    Decoration = 2,
    /// Сюжетная декорация: удалённые/скрытые объекты
    /// ([`PLOT_PROP_UNIT_TYPES`], 3).
    PlotProp = 3,
}

impl ObjectKind {
    /// Вид типа объекта по [`UnitType::tag`] — тотальная биекция
    /// `0x00–0x5C` (93 типа, без пропусков и дублей).
    ///
    /// Единственный источник — три списка исключений
    /// ([`EFFECT_UNIT_TYPES`], [`DECORATION_UNIT_TYPES`],
    /// [`PLOT_PROP_UNIT_TYPES`]); всё остальное — [`Self::Unit`]. Тотальность и
    /// взаимоисключение проверяются тестами по [`UnitType::ALL`].
    #[inline]
    pub fn classify(unit_type: UnitType) -> Self {
        // Эффекты (14) — снаряды, взрывы, обломки, следы.
        if EFFECT_UNIT_TYPES.contains(&unit_type) {
            Self::Effect
        } else if DECORATION_UNIT_TYPES.contains(&unit_type) {
            Self::Decoration
        } else if PLOT_PROP_UNIT_TYPES.contains(&unit_type) {
            Self::PlotProp
        } else {
            Self::Unit
        }
    }

    /// Юнит ли вид.
    #[inline]
    pub fn is_unit(self) -> bool {
        matches!(self, Self::Unit)
    }

    /// Эффект ли вид.
    #[inline]
    pub fn is_effect(self) -> bool {
        matches!(self, Self::Effect)
    }

    /// Декорация ли вид.
    #[inline]
    pub fn is_decoration(self) -> bool {
        matches!(self, Self::Decoration)
    }

    /// Сюжетная декорация ли вид.
    #[inline]
    pub fn is_plot_prop(self) -> bool {
        matches!(self, Self::PlotProp)
    }
}

// ---------------------------------------------------------------------------
// Классификация типа юнита (per-type)
// ---------------------------------------------------------------------------

/// Приватный супертип-модуль sealed-типажа: реализовать [`Sealed`] (а значит и
/// [`UnitTypeClassify`]) можно только внутри этого крейта.
mod sealed {
    /// Маркер «свой» для `UnitType` (см. [`super::UnitTypeClassify`]).
    pub trait Sealed {}

    impl Sealed for max_types::enums::UnitType {}
}

/// Классификация [`UnitType`] — extension-типаж (правило сироты, Ф5.2).
///
/// `UnitType` объявлен в нижнем слое (`max_types::enums`), а эти методы читают
/// данные `max_core` (`UNIT_TYPE_PROPERTIES`, `BASE_UNIT_VALUES`, списки
/// `TURRET_`/`PRODUCING_`/`EFFECT_`/`DECORATION_`/`PLOT_PROP_UNIT_TYPES`),
/// поэтому они не могут остаться inherent-`impl` этого крейта.
///
/// Типаж **sealed** ([`sealed::Sealed`] — приватный супертип): downstream
/// крейты могут вызывать методы, но не могут реализовать типаж для своих типов.
///
/// Импорт — **явный**, в месте вызова (`use max_core::UnitTypeClassify;`),
/// никаких глоб-реэкспортов. `UnitType::tag()` остаётся inherent и едет с типом.
pub trait UnitTypeClassify: sealed::Sealed {
    /// Боевая роль типа ([`CombatRole::classify`]).
    fn combat_role(self) -> CombatRole;

    /// Форма типа ([`Mobility::classify`]).
    fn mobility(self) -> Mobility;

    /// Футпринт типа ([`Footprint::classify`]): 1×1 или 2×2.
    fn footprint(self) -> Footprint;

    /// Занимает ли тип 2×2 (`BUILDING`). Лайаут, см. [`Footprint`].
    fn is_quad(self) -> bool;

    /// Среда типа ([`Environment::classify`]).
    fn environment(self) -> Option<Environment>;

    /// Вид груза типа ([`CargoKind::classify`]).
    fn cargo_kind(self) -> CargoKind;

    /// Вид объекта типа ([`ObjectKind::classify`]): юнит / эффект / декорация /
    /// сюжетная декорация. `UnitType` при этом не меняется (Ф4).
    fn object_kind(self) -> ObjectKind;

    /// Реальный юнит или здание (`ObjectKind::Unit`, 70 типов).
    fn is_unit(self) -> bool;

    /// Эффект (`ObjectKind::Effect`): снаряд, взрыв, обломок, след (14).
    fn is_effect(self) -> bool;

    /// Декорация/стройплощадка (`ObjectKind::Decoration`, 6).
    fn is_decoration(self) -> bool;

    /// Сюжетная декорация (`ObjectKind::PlotProp`): `MASTER`, `HARVSTER`,
    /// `WALDO`.
    fn is_plot_prop(self) -> bool;

    /// Производящий ли тип (`Cargo_GetNetProduction`, [`PRODUCING_UNIT_TYPES`]).
    fn is_producing(self) -> bool;

    /// Воздушная среда (`MOBILE_AIR_UNIT`) — [`Environment::Air`].
    fn is_air(self) -> bool;

    /// Наземная среда (`MOBILE_LAND_UNIT` без `MOBILE_SEA_UNIT`) —
    /// [`Environment::Ground`].
    fn is_ground(self) -> bool;

    /// Морская среда (`MOBILE_SEA_UNIT` без `MOBILE_LAND_UNIT`) —
    /// [`Environment::Sea`].
    fn is_sea(self) -> bool;

    /// Наземно-морская среда (`MOBILE_SEA_UNIT | MOBILE_LAND_UNIT`) —
    /// [`Environment::Amphibious`].
    fn is_amphibious(self) -> bool;

    /// Мобильный (`STATIONARY` снят) — [`Mobility::Mobile`].
    fn is_mobile(self) -> bool;

    /// Здание — [`Mobility::Building`] (`BUILDING` либо турель).
    ///
    /// **Не эквивалент бита `BUILDING`**: турели — здания по решению
    /// владельца, хотя флаг `BUILDING` (`0x10`, «2×2-структура») у них снят.
    /// Там, где проверяется именно раскладка 2×2, используется [`Footprint`]
    /// ([`is_quad()`](Self::is_quad)).
    fn is_building(self) -> bool;

    /// Стационарный-прочий ([`Mobility::Stationary`]): `STATIONARY` без
    /// `BUILDING`, не турель.
    ///
    /// Не путать с «неподвижным вообще» (здание или стационарный-прочий): для
    /// этого — `!is_mobile()`, что эквивалентно биту `STATIONARY`.
    fn is_stationary(self) -> bool;

    /// Боевой ([`CombatRole::Combat`]): `attack > 0`.
    fn is_combat(self) -> bool;
}

impl UnitTypeClassify for UnitType {
    /// Боевая роль типа — [`CombatRole::classify`].
    #[inline]
    fn combat_role(self) -> CombatRole {
        CombatRole::classify(self)
    }

    /// Форма типа — [`Mobility::classify`].
    #[inline]
    fn mobility(self) -> Mobility {
        Mobility::classify(self)
    }

    /// Футпринт типа — [`Footprint::classify`].
    #[inline]
    fn footprint(self) -> Footprint {
        Footprint::classify(self)
    }

    /// Занимает ли тип 2×2 (`BUILDING`).
    #[inline]
    fn is_quad(self) -> bool {
        self.footprint().is_quad()
    }

    /// Среда типа — [`Environment::classify`].
    #[inline]
    fn environment(self) -> Option<Environment> {
        Environment::classify(self)
    }

    /// Вид груза типа — [`CargoKind::classify`].
    #[inline]
    fn cargo_kind(self) -> CargoKind {
        CargoKind::classify(self)
    }

    /// Вид объекта типа — [`ObjectKind::classify`].
    #[inline]
    fn object_kind(self) -> ObjectKind {
        ObjectKind::classify(self)
    }

    /// Реальный юнит или здание (`ObjectKind::Unit`, 70 типов).
    #[inline]
    fn is_unit(self) -> bool {
        self.object_kind() == ObjectKind::Unit
    }

    /// Эффект (`ObjectKind::Effect`): снаряд, взрыв, обломок, след (14).
    #[inline]
    fn is_effect(self) -> bool {
        self.object_kind() == ObjectKind::Effect
    }

    /// Декорация/стройплощадка (`ObjectKind::Decoration`, 6).
    #[inline]
    fn is_decoration(self) -> bool {
        self.object_kind() == ObjectKind::Decoration
    }

    /// Сюжетная декорация (`ObjectKind::PlotProp`): `MASTER`, `HARVSTER`,
    /// `WALDO`.
    #[inline]
    fn is_plot_prop(self) -> bool {
        self.object_kind() == ObjectKind::PlotProp
    }

    /// Производящий ли тип (`Cargo_GetNetProduction`).
    #[inline]
    fn is_producing(self) -> bool {
        PRODUCING_UNIT_TYPES.contains(&self)
    }

    /// Воздушная среда — [`Environment::Air`].
    ///
    /// Точный эквивалент проверки бита `flags & MOBILE_AIR_UNIT != 0`:
    /// `Environment::classify_flags` ставит `Air` первым и только по этому
    /// биту, поэтому других источников `Environment::Air` нет.
    #[inline]
    fn is_air(self) -> bool {
        self.environment() == Some(Environment::Air)
    }

    /// Наземная среда — [`Environment::Ground`].
    ///
    /// Амфибийные типы сюда **не** входят: для бита `MOBILE_LAND_UNIT`
    /// (включая амфибии) условие — `is_ground() || is_amphibious()`.
    #[inline]
    fn is_ground(self) -> bool {
        self.environment() == Some(Environment::Ground)
    }

    /// Морская среда — [`Environment::Sea`].
    ///
    /// Амфибийные типы сюда **не** входят: для бита `MOBILE_SEA_UNIT`
    /// (включая амфибии) условие — `is_sea() || is_amphibious()`.
    #[inline]
    fn is_sea(self) -> bool {
        self.environment() == Some(Environment::Sea)
    }

    /// Наземно-морская среда — [`Environment::Amphibious`].
    #[inline]
    fn is_amphibious(self) -> bool {
        self.environment() == Some(Environment::Amphibious)
    }

    /// Мобильный (`STATIONARY` снят) — [`Mobility::Mobile`].
    #[inline]
    fn is_mobile(self) -> bool {
        self.mobility() == Mobility::Mobile
    }

    /// Здание — [`Mobility::Building`] (`BUILDING` либо турель).
    #[inline]
    fn is_building(self) -> bool {
        self.mobility() == Mobility::Building
    }

    /// Стационарный-прочий — [`Mobility::Stationary`].
    #[inline]
    fn is_stationary(self) -> bool {
        self.mobility() == Mobility::Stationary
    }

    /// Боевой — [`CombatRole::Combat`] (`attack > 0`).
    #[inline]
    fn is_combat(self) -> bool {
        self.combat_role().is_combat()
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

    /// Футпринт экземпляра по его флагам ([`Footprint::classify_unit`]).
    #[inline]
    pub fn footprint(&self) -> Footprint {
        Footprint::classify_unit(self)
    }

    /// Занимает ли экземпляр 2×2 (`BUILDING` в его флагах).
    #[inline]
    pub fn is_quad(&self) -> bool {
        self.footprint().is_quad()
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

    /// Воздушная среда экземпляра по его флагам — [`UnitType::is_air`].
    #[inline]
    pub fn is_air(&self) -> bool {
        self.environment() == Some(Environment::Air)
    }

    /// Наземная среда экземпляра по его флагам — [`UnitType::is_ground`].
    #[inline]
    pub fn is_ground(&self) -> bool {
        self.environment() == Some(Environment::Ground)
    }

    /// Морская среда экземпляра по его флагам — [`UnitType::is_sea`].
    #[inline]
    pub fn is_sea(&self) -> bool {
        self.environment() == Some(Environment::Sea)
    }

    /// Наземно-морская среда экземпляра — [`UnitType::is_amphibious`].
    #[inline]
    pub fn is_amphibious(&self) -> bool {
        self.environment() == Some(Environment::Amphibious)
    }

    /// Мобильный ли экземпляр (`STATIONARY` снят) — [`UnitType::is_mobile`].
    #[inline]
    pub fn is_mobile(&self) -> bool {
        self.mobility() == Mobility::Mobile
    }

    /// Здание ли экземпляр — [`UnitType::is_building`] (турели — здания).
    #[inline]
    pub fn is_building(&self) -> bool {
        self.mobility() == Mobility::Building
    }

    /// Стационарный-прочий ли экземпляр — [`UnitType::is_stationary`].
    #[inline]
    pub fn is_stationary(&self) -> bool {
        self.mobility() == Mobility::Stationary
    }

    /// Боевой ли экземпляр (по его `base_values`) — [`UnitType::is_combat`].
    #[inline]
    pub fn is_combat(&self) -> bool {
        self.combat_role().is_combat()
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

    /// Ожидаемый футпринт по флагу `BUILDING` (`object-classifier.md`).
    fn expected_footprint(unit_type: UnitType) -> Footprint {
        if has_flag(unit_type, BUILDING) {
            Footprint::Quad
        } else {
            Footprint::Single
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
    fn footprint_matches_building_bit_for_all_93_types() {
        let (mut quad, mut single) = (0, 0);

        for &unit_type in UnitType::ALL {
            let expected = expected_footprint(unit_type);

            assert_eq!(
                Footprint::classify(unit_type),
                expected,
                "{unit_type:?}: flags = {:#010X}",
                unit_type_flags(unit_type)
            );
            assert_eq!(unit_type.footprint(), expected, "{unit_type:?}");
            assert_eq!(unit_type.is_quad(), expected.is_quad(), "{unit_type:?}");
            assert_eq!(
                Footprint::classify_flags(unit_type, unit_type_flags(unit_type)),
                expected,
                "{unit_type:?}"
            );

            if expected.is_quad() {
                quad += 1;
            } else {
                single += 1;
            }
        }

        // 26 зданий по `Mobility`, из них у 4 турелей `BUILDING` снят: 22 + 71.
        assert_eq!((quad, single), (22, 71));

        // Стабильные теги сериализации — явные дискриминанты, не порядок.
        assert_eq!(Footprint::Single as u8, 0);
        assert_eq!(Footprint::Quad as u8, 1);
    }

    #[test]
    fn footprint_quad_implies_mobility_building_for_all_93_types() {
        for &unit_type in UnitType::ALL {
            if unit_type.is_quad() {
                assert_eq!(
                    Mobility::classify(unit_type),
                    Mobility::Building,
                    "{unit_type:?}: Footprint::Quad без Mobility::Building"
                );
            }
        }

        // Обратное неверно: турели — `Building`, но `Single`.
        assert!(TURRET_UNIT_TYPES.iter().any(|turret| turret.is_building()));
    }

    #[test]
    fn turrets_are_single_footprint_but_building_mobility() {
        for &turret in &TURRET_UNIT_TYPES {
            assert_eq!(
                turret.footprint(),
                Footprint::Single,
                "{turret:?}: турель занимает 1×1"
            );
            assert!(!turret.is_quad(), "{turret:?}");
            assert_eq!(
                Mobility::classify(turret),
                Mobility::Building,
                "{turret:?}: турель — здание семантически"
            );
            assert!(!has_flag(turret, BUILDING), "{turret:?}");
        }
    }

    #[test]
    fn footprint_control_cases_for_decorations() {
        // 2×2-декорации несут `BUILDING` (`SC_UNITS`) — это `Quad`.
        for decoration in [
            UnitType::LargeRubble,
            UnitType::LargeTape,
            UnitType::LargeSlab,
            UnitType::LargeCones,
        ] {
            assert_eq!(
                decoration.footprint(),
                Footprint::Quad,
                "{decoration:?}: {:#010X}",
                unit_type_flags(decoration)
            );
            assert!(decoration.is_quad(), "{decoration:?}");
        }

        // 1×1-декорации — `Single`.
        for decoration in [
            UnitType::SmallRubble,
            UnitType::SmallTape,
            UnitType::SmallSlab,
            UnitType::SmallCones,
        ] {
            assert_eq!(
                decoration.footprint(),
                Footprint::Single,
                "{decoration:?}: {:#010X}",
                unit_type_flags(decoration)
            );
            assert!(decoration.footprint().is_single(), "{decoration:?}");
        }

        // Экземпляр: футпринт — по флагам экземпляра.
        let mut mine = UnitInfo::new(UnitType::MiningStation, TeamId::RED, UnitId::NO_ID);
        mine.flags = unit_type_flags(UnitType::MiningStation);
        assert!(mine.is_quad());
        mine.flags &= !BUILDING;
        assert!(!mine.is_quad());
        assert!(mine.footprint().is_single());
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

    #[test]
    fn ergonomic_helpers_agree_with_axes_for_all_93_types() {
        let mut environment_counts = [0usize; 4];
        let mut mobile = 0;

        for &unit_type in UnitType::ALL {
            let environment = unit_type.environment();
            let mobility = unit_type.mobility();

            assert_eq!(unit_type.is_air(), environment == Some(Environment::Air));
            assert_eq!(
                unit_type.is_ground(),
                environment == Some(Environment::Ground)
            );
            assert_eq!(unit_type.is_sea(), environment == Some(Environment::Sea));
            assert_eq!(
                unit_type.is_amphibious(),
                environment == Some(Environment::Amphibious)
            );
            assert_eq!(unit_type.is_mobile(), mobility == Mobility::Mobile);
            assert_eq!(unit_type.is_building(), mobility == Mobility::Building);
            assert_eq!(unit_type.is_stationary(), mobility == Mobility::Stationary);
            assert_eq!(
                unit_type.is_combat(),
                unit_type.combat_role() == CombatRole::Combat
            );

            // Среды взаимоисключающие: одновременно не более одной.
            let flags = [
                unit_type.is_air(),
                unit_type.is_ground(),
                unit_type.is_sea(),
                unit_type.is_amphibious(),
            ];
            assert!(
                flags.iter().filter(|set| **set).count() <= 1,
                "{unit_type:?}"
            );

            if let Some(environment) = environment {
                environment_counts[match environment {
                    Environment::Air => 0,
                    Environment::Ground => 1,
                    Environment::Sea => 2,
                    Environment::Amphibious => 3,
                }] += 1;
                assert!(unit_type.is_mobile(), "{unit_type:?}");
            }

            if mobility == Mobility::Mobile {
                mobile += 1;
            }
        }

        assert_eq!(environment_counts, [5, 17, 9, 5]);
        assert_eq!(mobile, 43);

        // Бит `STATIONARY` эквивалентен `!is_mobile()`, если соблюдён
        // инвариант `BUILDING ⟹ STATIONARY`.
        for &unit_type in UnitType::ALL {
            assert_eq!(
                !unit_type.is_mobile(),
                has_flag(unit_type, STATIONARY),
                "{unit_type:?}"
            );
        }

        // Турель — здание по решению владельца, но бит `BUILDING` у неё снят:
        // это разные вопросы (`is_building()` ≠ `flags & BUILDING`).
        assert!(UnitType::GunTurret.is_building());
        assert!(!has_flag(UnitType::GunTurret, BUILDING));
    }

    #[test]
    fn instance_helpers_follow_instance_flags() {
        let mut tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId::NO_ID);
        tank.flags = unit_type_flags(UnitType::Tank);
        assert!(!tank.is_air());
        assert!(tank.is_ground());
        assert!(tank.is_mobile());
        assert!(!tank.is_building());
        assert!(tank.is_combat());

        // Снятые `STATIONARY`/`BUILDING` у экземпляра здания — экземпляр
        // мобильный, тип остаётся зданием.
        let mut barracks = UnitInfo::new(UnitType::Barracks, TeamId::RED, UnitId::NO_ID);
        barracks.flags = unit_type_flags(UnitType::Barracks);
        assert!(barracks.is_building());
        assert!(!barracks.is_mobile());
        barracks.flags &= !(BUILDING | STATIONARY);
        assert!(!barracks.is_building());
        assert!(barracks.is_mobile());

        // Воздушные флаги вместо наземных — экземпляр «воздушный».
        tank.flags = (tank.flags & !(MOBILE_SEA_UNIT | MOBILE_LAND_UNIT)) | MOBILE_AIR_UNIT;
        assert!(tank.is_air());
        assert!(!tank.is_ground());
    }

    // -----------------------------------------------------------------------
    // Вид объекта (Ф4): юнит / эффект / декорация / сюжетная декорация
    // -----------------------------------------------------------------------

    /// Токены эффектов — дословно из `object-classifier.md`, «Не-юниты».
    const EFFECT_TOKENS: [&str; 14] = [
        "ROCKET", "TORPEDO", "ALNMISSL", "ALNTBALL", "ALNABALL", "LNDEXPLD", "AIREXPLD",
        "SEAEXPLD", "BLDEXPLD", "HITEXPLD", "LRGRUBLE", "SMLRUBLE", "RKTSMOKE", "TRPBUBLE",
    ];

    /// Токены декораций — дословно из `object-classifier.md`, «Не-юниты».
    const DECORATION_TOKENS: [&str; 6] = [
        "LRGTAPE", "SMLTAPE", "LRGSLAB", "SMLSLAB", "LRGCONES", "SMLCONES",
    ];

    /// Токены сюжетных декораций — дословно из `object-classifier.md`.
    const PLOT_PROP_TOKENS: [&str; 3] = ["MASTER", "HARVSTER", "WALDO"];

    /// Независимый оракул: ожидаемый вид — по ресурсному токену [`UnitType::tag`].
    fn expected_object_kind(unit_type: UnitType) -> ObjectKind {
        let tag = unit_type.tag();

        if EFFECT_TOKENS.contains(&tag) {
            ObjectKind::Effect
        } else if DECORATION_TOKENS.contains(&tag) {
            ObjectKind::Decoration
        } else if PLOT_PROP_TOKENS.contains(&tag) {
            ObjectKind::PlotProp
        } else {
            ObjectKind::Unit
        }
    }

    #[test]
    fn object_kind_is_total_bijection_over_93_types() {
        let mut counts = [0usize; 4];

        for &unit_type in UnitType::ALL {
            let expected = expected_object_kind(unit_type);
            let actual = unit_type.object_kind();

            assert_eq!(
                actual,
                expected,
                "{:?}: tag = {}, flags = {:#010X}",
                unit_type,
                unit_type.tag(),
                unit_type_flags(unit_type)
            );
            assert_eq!(ObjectKind::classify(unit_type), expected, "{unit_type:?}");

            counts[actual as usize] += 1;
        }

        // 70 юнитов + 14 эффектов + 6 декораций + 3 сюжетных = 93 (тотальная
        // биекция, нет пропусков и дублей).
        assert_eq!(counts, [70, 14, 6, 3]);
        assert_eq!(counts.iter().sum::<usize>(), UnitType::ALL.len());
        assert_eq!(counts.iter().sum::<usize>(), 93);

        // Списки исключений — без дублей и ровно объявленной длины.
        assert_eq!(EFFECT_UNIT_TYPES.len(), 14);
        assert_eq!(DECORATION_UNIT_TYPES.len(), 6);
        assert_eq!(PLOT_PROP_UNIT_TYPES.len(), 3);

        for (name, list) in [
            ("EFFECT_UNIT_TYPES", &EFFECT_UNIT_TYPES[..]),
            ("DECORATION_UNIT_TYPES", &DECORATION_UNIT_TYPES[..]),
            ("PLOT_PROP_UNIT_TYPES", &PLOT_PROP_UNIT_TYPES[..]),
        ] {
            for (index, unit_type) in list.iter().enumerate() {
                assert_eq!(
                    list.iter().filter(|other| *other == unit_type).count(),
                    1,
                    "{name}: дубль {unit_type:?} (позиция {index})"
                );
                assert!(
                    UnitType::ALL.contains(unit_type),
                    "{name}: {unit_type:?} вне `UnitType::ALL`"
                );
            }
        }

        // Границы диапазона юнитов по токенам: `COMMTWR`..`ALNPLANE`.
        assert_eq!(UnitType::GoldRefinery.tag(), "COMMTWR");
        assert_eq!(UnitType::AlienPlane.tag(), "ALNPLANE");
        assert_eq!(UnitType::GoldRefinery.object_kind(), ObjectKind::Unit);
        assert_eq!(UnitType::AlienPlane.object_kind(), ObjectKind::Unit);
    }

    #[test]
    fn object_kind_variants_are_mutually_exclusive() {
        // Виды различаются: `Unit` не равен остальным.
        for other in [
            ObjectKind::Effect,
            ObjectKind::Decoration,
            ObjectKind::PlotProp,
        ] {
            assert_ne!(ObjectKind::Unit, other, "{other:?}");
            assert!(!other.is_unit(), "{other:?}");
        }
        assert!(ObjectKind::Unit.is_unit());
        assert!(!ObjectKind::Unit.is_effect());
        assert!(!ObjectKind::Unit.is_decoration());
        assert!(!ObjectKind::Unit.is_plot_prop());

        // У каждого из 93 типов срабатывает ровно один помощник.
        for &unit_type in UnitType::ALL {
            let kind = unit_type.object_kind();
            let fired = [
                unit_type.is_unit(),
                unit_type.is_effect(),
                unit_type.is_decoration(),
                unit_type.is_plot_prop(),
            ];

            assert_eq!(
                fired.iter().filter(|set| **set).count(),
                1,
                "{unit_type:?}: {fired:?}"
            );
            assert_eq!(
                unit_type.is_unit(),
                kind == ObjectKind::Unit,
                "{unit_type:?}"
            );
            assert_eq!(
                unit_type.is_effect(),
                kind == ObjectKind::Effect,
                "{unit_type:?}"
            );
            assert_eq!(
                unit_type.is_decoration(),
                kind == ObjectKind::Decoration,
                "{unit_type:?}"
            );
            assert_eq!(
                unit_type.is_plot_prop(),
                kind == ObjectKind::PlotProp,
                "{unit_type:?}"
            );
            assert_eq!(kind.is_unit(), kind == ObjectKind::Unit, "{unit_type:?}");
        }
    }

    #[test]
    fn object_kind_control_cases() {
        // (в) Юниты: производящее здание, шахта, боевой мобильный.
        for unit_type in [
            UnitType::GoldRefinery,
            UnitType::MiningStation,
            UnitType::Tank,
        ] {
            assert_eq!(
                unit_type.object_kind(),
                ObjectKind::Unit,
                "{unit_type:?}: {}",
                unit_type.tag()
            );
            assert!(unit_type.is_unit(), "{unit_type:?}");
        }

        // Эффекты: взрыв, след, обломок.
        for effect in [
            UnitType::LandExplosion,
            UnitType::RocketSmoke,
            UnitType::LargeRubble,
        ] {
            assert_eq!(
                effect.object_kind(),
                ObjectKind::Effect,
                "{effect:?}: {}",
                effect.tag()
            );
            assert!(effect.is_effect(), "{effect:?}");
            assert!(!effect.is_unit(), "{effect:?}");
        }

        // Декорация: стройплощадка 2×2.
        assert_eq!(UnitType::LargeTape.object_kind(), ObjectKind::Decoration);
        assert!(UnitType::LargeTape.is_decoration());
        assert!(!UnitType::LargeTape.is_unit());

        // Сюжетные декорации: `MASTER`, `HARVSTER`, `WALDO`.
        for plot_prop in [
            UnitType::MasterBuilder,
            UnitType::Harvester,
            UnitType::DeadWaldo,
        ] {
            assert_eq!(
                plot_prop.object_kind(),
                ObjectKind::PlotProp,
                "{plot_prop:?}: {}",
                plot_prop.tag()
            );
            assert!(plot_prop.is_plot_prop(), "{plot_prop:?}");
            assert!(!plot_prop.is_unit(), "{plot_prop:?}");
        }
    }

    #[test]
    fn object_kind_discriminants_are_stable() {
        // Стабильные теги сериализации — явные дискриминанты, не порядок.
        assert_eq!(ObjectKind::Unit as u8, 0);
        assert_eq!(ObjectKind::Effect as u8, 1);
        assert_eq!(ObjectKind::Decoration as u8, 2);
        assert_eq!(ObjectKind::PlotProp as u8, 3);
    }
}
