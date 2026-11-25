//! Type-safe unit classification layer — "kind" over bit flags (F2).
//!
//! The layer **changes nothing**: `UnitInfo::flags` remains serializable
//! state (snapshot/CRC — rule 1.3 of `rust-programming-rules.md`), and everything
//! below is derived values computable from already existing data:
//!
//! * [`UnitType`] — from the [`crate::access::UNIT_TYPE_PROPERTIES`] table
//!   (`SC_UNITS`) and [`crate::unit_values::BASE_UNIT_VALUES`] (`SC_ATTRI`);
//! * [`UnitInfo`] — from the **instance** flags (`unit.flags`): at runtime they
//!   may differ from the type table (`HOVERING`, `EXPLODING`, cleared
//!   `STATIONARY`), so the instance has its own classification.
//!
//! The taxonomy source is `max-brh-docs/object-classifier.md` ("Key
//! criteria", "Unit classification axes", "Non-units").
//!
//! # Separation of non-units (F4)
//!
//! [`ObjectKind`] separates **units** from **non-units** (effects, decorations,
//! plot objects) at the semantic level. [`UnitType`] itself **does not
//! change**: the 93 variants with discriminants `0x00–0x5C` remain `ResourceID`
//! (`SC_UNITS`) — CRC, the replay digest and the V70 format depend on them. `ObjectKind`
//! is a derived value, like [`Footprint`].
//!
//! Axes:
//!
//! | Axis | Values | Rust |
//! |---|---|---|
//! | Object kind | unit / effect / decoration / plot decoration | [`ObjectKind`] |
//! | Combat role | combat / non-combat | [`CombatRole`] |
//! | Form | building / mobile / stationary-other | [`Mobility`] |
//! | Footprint (map size) | 1×1 / 2×2 | [`Footprint`] |
//! | Mobile environment | air / amphibious / ground / sea | [`Environment`] |
//! | Production | producing / non-producing | [`UnitType::is_producing`] |
//! | Cargo | none / resource / unit | [`CargoKind`] |
//!
//! # Decision on marker traits (variant C)
//!
//! Marker traits without implementations are **not used** (rule 1, the owner's
//! decision): `UnitType` is a closed `enum` of 93 variants, and `impl Combat for
//! UnitType` would claim "combat" for all 93 at once — dead code. The
//! classification is carried by the `enum` (the single source of truth); behavior — by behavioral
//! traits (`CanMove`, `CanAttack`) in generic code, not category markers.
//!
//! C++ source: `enums.hpp` (`BUILDING`, `STATIONARY`, `MOBILE_*_UNIT`),
//! `units.cpp` (`Units::LoadResource` — `SC_UNITS` flags), `attributes.cpp`
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
// Domain data: exception lists
// ---------------------------------------------------------------------------

/// Turrets are **semantically buildings**, although the `BUILDING` flag (`0x10`) is
/// not set: in `SC_UNITS` they are 1×1 `STATIONARY`. The owner's decision
/// (`object-classifier.md`, "Key criteria").
///
/// C++: `GUNTURRT`/`ARTYTRRT`/`ANTIAIR`/`ANTIMSSL` (`SC_UNITS`).
pub const TURRET_UNIT_TYPES: [UnitType; 4] = [
    UnitType::GunTurret,
    UnitType::Artillery,
    UnitType::AntiAir,
    UnitType::MissileLauncher,
];

/// Producing types — exactly those handled by `Cargo_GetNetProduction`
/// (`cargo.cpp:70`, 12 types): 5 unit factories + 6 resource buildings + the mine.
///
/// C++: `Cargo_GetNetProduction` (`cargo.cpp:70`); `object-classifier.md`,
/// "Production units".
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

/// Whether the type is a turret (see [`TURRET_UNIT_TYPES`]).
#[inline]
pub fn is_turret(unit_type: UnitType) -> bool {
    TURRET_UNIT_TYPES.contains(&unit_type)
}

// ---------------------------------------------------------------------------
// Domain data: non-units (F4)
// ---------------------------------------------------------------------------

/// Effects are **non-units**: projectiles, explosions, rubble, trails (14 types).
///
/// Source — `object-classifier.md`, "Non-units" → "Effect":
/// `ROCKET`, `TORPEDO`, `ALNMISSL`, `ALNTBALL`, `ALNABALL` (projectiles);
/// `LNDEXPLD`, `AIREXPLD`, `SEAEXPLD`, `BLDEXPLD`, `HITEXPLD` (explosions);
/// `LRGRUBLE`, `SMLRUBLE` (rubble); `RKTSMOKE`, `TRPBUBLE` (trails).
///
/// C++: there is no separate predicate — they are all `ResourceID`s from `SC_UNITS`;
/// the separation is semantic (F4), `UnitType` does not change.
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

/// Decorations/build sites are **non-units** (6 types): `LRGTAPE`, `SMLTAPE`,
/// `LRGSLAB`, `SMLSLAB`, `LRGCONES`, `SMLCONES`.
///
/// Source — `object-classifier.md`, "Non-units" → "Decoration/build site".
pub const DECORATION_UNIT_TYPES: [UnitType; 6] = [
    UnitType::LargeTape,
    UnitType::SmallTape,
    UnitType::LargeSlab,
    UnitType::SmallSlab,
    UnitType::LargeCones,
    UnitType::SmallCones,
];

/// Plot decorations (removed/hidden) are **non-units** (3 types): `MASTER`,
/// `HARVSTER`, `WALDO`.
///
/// Source — `object-classifier.md`, "Non-units" → "Plot decoration".
pub const PLOT_PROP_UNIT_TYPES: [UnitType; 3] = [
    UnitType::MasterBuilder,
    UnitType::Harvester,
    UnitType::DeadWaldo,
];

// ---------------------------------------------------------------------------
// Combat role
// ---------------------------------------------------------------------------

/// Unit combat role — the `object-classifier.md` axis "Combat / Non-combat".
///
/// C++: `attack` (`ATTRIB_ATTACK`) from `SC_ATTRI` (`Attributes::LoadResource`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CombatRole {
    /// Combat: `attack > 0`.
    Combat,
    /// Non-combat: `attack = 0`.
    NonCombat,
}

impl CombatRole {
    /// Classifies a unit type by the base `attack` attribute.
    #[inline]
    pub fn classify(unit_type: UnitType) -> Self {
        Self::from_attack(base_unit_values(unit_type).attack)
    }

    /// Classifies an instance by **its** attributes (`base_values`:
    /// clan modifiers and the upgrades of this team's type).
    #[inline]
    pub fn classify_unit(unit: &UnitInfo) -> Self {
        Self::from_attack(unit.base_values.attack)
    }

    /// `attack > 0` — combat; `attack = 0` — non-combat.
    #[inline]
    pub fn from_attack(attack: u16) -> Self {
        if attack > 0 {
            Self::Combat
        } else {
            Self::NonCombat
        }
    }

    /// Whether the type is combat.
    #[inline]
    pub fn is_combat(self) -> bool {
        matches!(self, Self::Combat)
    }
}

// ---------------------------------------------------------------------------
// Form
// ---------------------------------------------------------------------------

/// Unit form — the `object-classifier.md` axis "Building / Mobile /
/// Stationary-other".
///
/// C++: `UNIT_TYPE_PROPERTIES` (`SC_UNITS`, `Units::LoadResource`), the
/// `BUILDING` (`0x10`) and `STATIONARY` (`0x200`) flags; turrets are the exception
/// ([`TURRET_UNIT_TYPES`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mobility {
    /// Building: `BUILDING` is set (2×2 structure) or the type is a turret.
    Building,
    /// Mobile: `STATIONARY` is cleared (including projectiles — C++ sees them so too).
    Mobile,
    /// Stationary-other: `STATIONARY` without `BUILDING`, not a turret.
    Stationary,
}

impl Mobility {
    /// Classifies a type by `SC_UNITS` flags and the turret list.
    #[inline]
    pub fn classify(unit_type: UnitType) -> Self {
        Self::classify_flags(unit_type, unit_type_flags(unit_type))
    }

    /// Classifies an instance by instance flags (`unit.flags`); the type form
    /// is used only for the turret list.
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
// Footprint
// ---------------------------------------------------------------------------

/// Footprint (map size) — the `object-classifier.md` "Footprint" axis.
///
/// **Layout, not semantics:** the axis reads exactly the `BUILDING` bit (`0x10`,
/// "2×2 structure"). Turrets are [`Mobility::Building`] semantically, but their
/// flag is cleared, hence [`Footprint::Single`]; the invariant
/// [`Footprint::Quad`] `⟹` [`Mobility::Building`] holds (the converse does not).
///
/// Stable serialization tags are **explicit** discriminants (variant order is
/// not part of the contract): `Single = 0`, `Quad = 1`.
///
/// C++: `BUILDING` (`enums.hpp`); `Hash_MapHash::Add/Remove` (`hash.cpp:188`,
/// `hash.cpp:227`); the size `(flags & BUILDING) ? 2 : 1` (`units_manager.cpp:2056`,
/// `unitinfo.cpp:4086`, `task.cpp:314`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Footprint {
    /// 1×1 — one map cell (mobile units, turrets, 1×1 structures, effects).
    Single = 0,
    /// 2×2 — four map cells (the `BUILDING` flag).
    Quad = 1,
}

impl Footprint {
    /// Unit-type footprint by `SC_UNITS` flags.
    #[inline]
    pub fn classify(unit_type: UnitType) -> Self {
        Self::classify_flags(unit_type, unit_type_flags(unit_type))
    }

    /// Instance footprint by **its** flags (`unit.flags`): if the instance has
    /// `BUILDING` dynamically cleared, it occupies 1×1.
    #[inline]
    pub fn classify_unit(unit: &UnitInfo) -> Self {
        Self::classify_flags(unit.unit_type, unit.flags)
    }

    /// Footprint by the `BUILDING` flag (`0x10`): set — 2×2, cleared — 1×1.
    ///
    /// The `Quad ⟹ Mobility::Building` invariant is checked by `debug_assert!`: the
    /// `BUILDING` flag always yields [`Mobility::Building`], the converse is false (a turret is
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

    /// Footprint by raw flags when the type is unknown (layout APIs: `MapHash`,
    /// `Access_*` take `flags`). The only place in the axis that reads the bit.
    #[inline]
    pub fn from_flags(flags: u32) -> Self {
        if flags & BUILDING != 0 {
            Self::Quad
        } else {
            Self::Single
        }
    }

    /// Whether the footprint is 2×2.
    #[inline]
    pub fn is_quad(self) -> bool {
        matches!(self, Self::Quad)
    }

    /// Whether the footprint is 1×1.
    #[inline]
    pub fn is_single(self) -> bool {
        matches!(self, Self::Single)
    }
}

// ---------------------------------------------------------------------------
// Mobile environment
// ---------------------------------------------------------------------------

/// Mobile unit environment — the `object-classifier.md` axis "Air /
/// Amphibious / Ground / Sea".
///
/// C++: `MOBILE_AIR_UNIT` (`0x40`), `MOBILE_SEA_UNIT` (`0x80`),
/// `MOBILE_LAND_UNIT` (`0x100`) from `SC_UNITS`. `HOVERING` (`0x10000`) is **not an
/// environment** but a runtime flight state (`UnitInfo::Take`/`Land`), so it does not
/// participate in the classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Environment {
    /// Air: `MOBILE_AIR_UNIT`.
    Air,
    /// Land-sea: `MOBILE_SEA_UNIT | MOBILE_LAND_UNIT`.
    Amphibious,
    /// Ground: `MOBILE_LAND_UNIT` without `MOBILE_SEA_UNIT`.
    Ground,
    /// Sea: `MOBILE_SEA_UNIT` without `MOBILE_LAND_UNIT`.
    Sea,
}

impl Environment {
    /// Unit-type environment; `None` — no environment flags (stationary buildings,
    /// as well as projectiles/trails without `MOBILE_*`: `ROCKET`, `TORPEDO`, `ALNMISSL`,
    /// `ALNTBALL`, `ALNABALL`, `RKTSMOKE`, `TRPBUBLE`).
    #[inline]
    pub fn classify(unit_type: UnitType) -> Option<Self> {
        Self::classify_flags(unit_type_flags(unit_type))
    }

    /// Instance environment by its flags (`unit.flags`); `HOVERING` has no effect.
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
// Cargo
// ---------------------------------------------------------------------------

/// Unit cargo kind — the `object-classifier.md` "Cargo (type)" axis.
///
/// C++: `Unit::CargoType` (`unit.hpp`), `ResourceManager_GetUnit(type)
/// .GetCargoType()`; mutually exclusive `CARGO_TYPE_*` groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CargoKind {
    /// No cargo (`CARGO_TYPE_NONE`).
    None,
    /// Resource cargo (`RAW`/`FUEL`/`GOLD`).
    Resource,
    /// Unit cargo (`LAND`/`SEA`/`AIR`) — transport/hangar/dock.
    Unit,
}

impl CargoKind {
    /// Unit-type cargo kind via [`cargo_type_of`].
    #[inline]
    pub fn classify(unit_type: UnitType) -> Self {
        Self::from_cargo_type(cargo_type_of(unit_type))
    }

    /// Instance cargo kind: cargo is a **type** property, instance flags do not
    /// change it.
    #[inline]
    pub fn classify_unit(unit: &UnitInfo) -> Self {
        Self::classify(unit.unit_type)
    }

    /// Groups `CARGO_TYPE_*` into three kinds.
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
// Object kind: unit / non-unit (F4)
// ---------------------------------------------------------------------------

/// Object kind — semantic separation of **units** from **non-units** (F4).
///
/// This is a **derived** axis, like [`Footprint`]: [`UnitType`] itself remains
/// `ResourceID`-compatible (`#[repr(u16)]`, 93 variants, `0x00–0x5C`), because
/// its discriminants index the `SC_UNITS`/`SC_ATTRI` tables and enter the
/// CRC/replay digest and the V70 format. Reclassification happens **without changing
/// behavior**: effects and decorations remain objects of the same `ResourceID`s in
/// the world, but are no longer considered "units" by the logical classifier.
///
/// Stable serialization tags are **explicit** discriminants (variant order is
/// not part of the contract): `Unit = 0`, `Effect = 1`, `Decoration = 2`,
/// `PlotProp = 3`.
///
/// Source: `object-classifier.md`, sections "Units" and "Non-units".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ObjectKind {
    /// Unit: a real unit or building (70 types, `COMMTWR`..`ALNPLANE`).
    Unit = 0,
    /// Effect: projectile, explosion, rubble, trail ([`EFFECT_UNIT_TYPES`], 14).
    Effect = 1,
    /// Decoration/build site ([`DECORATION_UNIT_TYPES`], 6).
    Decoration = 2,
    /// Plot decoration: removed/hidden objects
    /// ([`PLOT_PROP_UNIT_TYPES`], 3).
    PlotProp = 3,
}

impl ObjectKind {
    /// Object type kind via [`UnitType::tag`] — a total bijection
    /// over `0x00–0x5C` (93 types, without gaps or duplicates).
    ///
    /// The only source is the three exception lists
    /// ([`EFFECT_UNIT_TYPES`], [`DECORATION_UNIT_TYPES`],
    /// [`PLOT_PROP_UNIT_TYPES`]); everything else is [`Self::Unit`]. Totality and
    /// mutual exclusivity are checked by tests over [`UnitType::ALL`].
    #[inline]
    pub fn classify(unit_type: UnitType) -> Self {
        // Effects (14) — projectiles, explosions, rubble, trails.
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

    /// Whether the kind is a unit.
    #[inline]
    pub fn is_unit(self) -> bool {
        matches!(self, Self::Unit)
    }

    /// Whether the kind is an effect.
    #[inline]
    pub fn is_effect(self) -> bool {
        matches!(self, Self::Effect)
    }

    /// Whether the kind is a decoration.
    #[inline]
    pub fn is_decoration(self) -> bool {
        matches!(self, Self::Decoration)
    }

    /// Whether the kind is a plot decoration.
    #[inline]
    pub fn is_plot_prop(self) -> bool {
        matches!(self, Self::PlotProp)
    }
}

// ---------------------------------------------------------------------------
// Unit-type classification (per-type)
// ---------------------------------------------------------------------------

/// Private supertrait module of the sealed trait: [`Sealed`] (and thus
/// [`UnitTypeClassify`]) can be implemented only inside this crate.
mod sealed {
    /// "Own" marker for `UnitType` (see [`super::UnitTypeClassify`]).
    pub trait Sealed {}

    impl Sealed for max_types::enums::UnitType {}
}

/// Classification of [`UnitType`] — an extension trait (orphan rule, F5.2).
///
/// `UnitType` is declared in the lower layer (`max_types::enums`), and these methods read
/// `max_core` data (`UNIT_TYPE_PROPERTIES`, `BASE_UNIT_VALUES`, the
/// `TURRET_`/`PRODUCING_`/`EFFECT_`/`DECORATION_`/`PLOT_PROP_UNIT_TYPES`),
/// so they cannot remain an inherent `impl` of this crate.
///
/// The trait is **sealed** ([`sealed::Sealed`] is a private supertrait): downstream
/// crates may call the methods but cannot implement the trait for their own types.
///
/// The import is **explicit**, at the call site (`use max_core::UnitTypeClassify;`),
/// no glob re-exports. `UnitType::tag()` remains inherent and travels with the type.
//
// `self` by value — as in the former `UnitType` inherent methods (`Copy` type,
// pass by value is free and keeps call sites unchanged). Clippy
// warns about `is_*`/`self` only for trait methods where the implementor's `Copy`
// is unknown; here it is an intentional exception, not a style violation.
#[allow(clippy::wrong_self_convention)]
pub trait UnitTypeClassify: sealed::Sealed {
    /// Type combat role ([`CombatRole::classify`]).
    fn combat_role(self) -> CombatRole;

    /// Type form ([`Mobility::classify`]).
    fn mobility(self) -> Mobility;

    /// Type footprint ([`Footprint::classify`]): 1×1 or 2×2.
    fn footprint(self) -> Footprint;

    /// Whether the type occupies 2×2 (`BUILDING`). Layout, see [`Footprint`].
    fn is_quad(self) -> bool;

    /// Type environment ([`Environment::classify`]).
    fn environment(self) -> Option<Environment>;

    /// Type cargo kind ([`CargoKind::classify`]).
    fn cargo_kind(self) -> CargoKind;

    /// Type object kind ([`ObjectKind::classify`]): unit / effect / decoration /
    /// plot decoration. `UnitType` does not change (F4).
    fn object_kind(self) -> ObjectKind;

    /// A real unit or building (`ObjectKind::Unit`, 70 types).
    fn is_unit(self) -> bool;

    /// Effect (`ObjectKind::Effect`): projectile, explosion, rubble, trail (14).
    fn is_effect(self) -> bool;

    /// Decoration/build site (`ObjectKind::Decoration`, 6).
    fn is_decoration(self) -> bool;

    /// Plot decoration (`ObjectKind::PlotProp`): `MASTER`, `HARVSTER`,
    /// `WALDO`.
    fn is_plot_prop(self) -> bool;

    /// Whether the type is producing (`Cargo_GetNetProduction`, [`PRODUCING_UNIT_TYPES`]).
    fn is_producing(self) -> bool;

    /// Air environment (`MOBILE_AIR_UNIT`) — [`Environment::Air`].
    fn is_air(self) -> bool;

    /// Ground environment (`MOBILE_LAND_UNIT` without `MOBILE_SEA_UNIT`) —
    /// [`Environment::Ground`].
    fn is_ground(self) -> bool;

    /// Sea environment (`MOBILE_SEA_UNIT` without `MOBILE_LAND_UNIT`) —
    /// [`Environment::Sea`].
    fn is_sea(self) -> bool;

    /// Land-sea environment (`MOBILE_SEA_UNIT | MOBILE_LAND_UNIT`) —
    /// [`Environment::Amphibious`].
    fn is_amphibious(self) -> bool;

    /// Mobile (`STATIONARY` cleared) — [`Mobility::Mobile`].
    fn is_mobile(self) -> bool;

    /// Building — [`Mobility::Building`] (`BUILDING` or a turret).
    ///
    /// **Not equivalent to the `BUILDING` bit**: turrets are buildings by the owner's
    /// decision, although their `BUILDING` flag (`0x10`, "2×2 structure") is cleared.
    /// Where the 2×2 layout specifically is checked, [`Footprint`] is used
    /// ([`is_quad()`](Self::is_quad)).
    fn is_building(self) -> bool;

    /// Stationary-other ([`Mobility::Stationary`]): `STATIONARY` without
    /// `BUILDING`, not a turret.
    ///
    /// Not to be confused with "immobile in general" (building or stationary-other): for
    /// that use `!is_mobile()`, which is equivalent to the `STATIONARY` bit.
    fn is_stationary(self) -> bool;

    /// Combat ([`CombatRole::Combat`]): `attack > 0`.
    fn is_combat(self) -> bool;
}

impl UnitTypeClassify for UnitType {
    /// Type combat role — [`CombatRole::classify`].
    #[inline]
    fn combat_role(self) -> CombatRole {
        CombatRole::classify(self)
    }

    /// Type form — [`Mobility::classify`].
    #[inline]
    fn mobility(self) -> Mobility {
        Mobility::classify(self)
    }

    /// Type footprint — [`Footprint::classify`].
    #[inline]
    fn footprint(self) -> Footprint {
        Footprint::classify(self)
    }

    /// Whether the type occupies 2×2 (`BUILDING`).
    #[inline]
    fn is_quad(self) -> bool {
        self.footprint().is_quad()
    }

    /// Type environment — [`Environment::classify`].
    #[inline]
    fn environment(self) -> Option<Environment> {
        Environment::classify(self)
    }

    /// Type cargo kind — [`CargoKind::classify`].
    #[inline]
    fn cargo_kind(self) -> CargoKind {
        CargoKind::classify(self)
    }

    /// Type object kind — [`ObjectKind::classify`].
    #[inline]
    fn object_kind(self) -> ObjectKind {
        ObjectKind::classify(self)
    }

    /// A real unit or building (`ObjectKind::Unit`, 70 types).
    #[inline]
    fn is_unit(self) -> bool {
        self.object_kind() == ObjectKind::Unit
    }

    /// Effect (`ObjectKind::Effect`): projectile, explosion, rubble, trail (14).
    #[inline]
    fn is_effect(self) -> bool {
        self.object_kind() == ObjectKind::Effect
    }

    /// Decoration/build site (`ObjectKind::Decoration`, 6).
    #[inline]
    fn is_decoration(self) -> bool {
        self.object_kind() == ObjectKind::Decoration
    }

    /// Plot decoration (`ObjectKind::PlotProp`): `MASTER`, `HARVSTER`,
    /// `WALDO`.
    #[inline]
    fn is_plot_prop(self) -> bool {
        self.object_kind() == ObjectKind::PlotProp
    }

    /// Whether the type is producing (`Cargo_GetNetProduction`).
    #[inline]
    fn is_producing(self) -> bool {
        PRODUCING_UNIT_TYPES.contains(&self)
    }

    /// Air environment — [`Environment::Air`].
    ///
    /// An exact equivalent of the `flags & MOBILE_AIR_UNIT != 0` check:
    /// `Environment::classify_flags` sets `Air` first and only by this
    /// bit, so there are no other sources of `Environment::Air`.
    #[inline]
    fn is_air(self) -> bool {
        self.environment() == Some(Environment::Air)
    }

    /// Ground environment — [`Environment::Ground`].
    ///
    /// Amphibious types do **not** fall here: for the `MOBILE_LAND_UNIT` bit
    /// (including amphibians) the condition is `is_ground() || is_amphibious()`.
    #[inline]
    fn is_ground(self) -> bool {
        self.environment() == Some(Environment::Ground)
    }

    /// Sea environment — [`Environment::Sea`].
    ///
    /// Amphibious types do **not** fall here: for the `MOBILE_SEA_UNIT` bit
    /// (including amphibians) the condition is `is_sea() || is_amphibious()`.
    #[inline]
    fn is_sea(self) -> bool {
        self.environment() == Some(Environment::Sea)
    }

    /// Land-sea environment — [`Environment::Amphibious`].
    #[inline]
    fn is_amphibious(self) -> bool {
        self.environment() == Some(Environment::Amphibious)
    }

    /// Mobile (`STATIONARY` cleared) — [`Mobility::Mobile`].
    #[inline]
    fn is_mobile(self) -> bool {
        self.mobility() == Mobility::Mobile
    }

    /// Building — [`Mobility::Building`] (`BUILDING` or a turret).
    #[inline]
    fn is_building(self) -> bool {
        self.mobility() == Mobility::Building
    }

    /// Stationary-other — [`Mobility::Stationary`].
    #[inline]
    fn is_stationary(self) -> bool {
        self.mobility() == Mobility::Stationary
    }

    /// Combat — [`CombatRole::Combat`] (`attack > 0`).
    #[inline]
    fn is_combat(self) -> bool {
        self.combat_role().is_combat()
    }
}

// ---------------------------------------------------------------------------
// Instance classification (per-instance)
// ---------------------------------------------------------------------------

impl UnitInfo {
    /// Instance combat role by its `base_values` ([`CombatRole::classify_unit`]).
    #[inline]
    pub fn combat_role(&self) -> CombatRole {
        CombatRole::classify_unit(self)
    }

    /// Instance form by its flags ([`Mobility::classify_unit`]).
    #[inline]
    pub fn mobility(&self) -> Mobility {
        Mobility::classify_unit(self)
    }

    /// Instance footprint by its flags ([`Footprint::classify_unit`]).
    #[inline]
    pub fn footprint(&self) -> Footprint {
        Footprint::classify_unit(self)
    }

    /// Whether the instance occupies 2×2 (`BUILDING` in its flags).
    #[inline]
    pub fn is_quad(&self) -> bool {
        self.footprint().is_quad()
    }

    /// Instance environment by its flags ([`Environment::classify_unit`]).
    #[inline]
    pub fn environment(&self) -> Option<Environment> {
        Environment::classify_unit(self)
    }

    /// Instance cargo kind ([`CargoKind::classify_unit`]).
    #[inline]
    pub fn cargo_kind(&self) -> CargoKind {
        CargoKind::classify_unit(self)
    }

    /// Whether the instance is producing (a type property).
    #[inline]
    pub fn is_producing(&self) -> bool {
        self.unit_type.is_producing()
    }

    /// Instance air environment by its flags — [`UnitType::is_air`].
    #[inline]
    pub fn is_air(&self) -> bool {
        self.environment() == Some(Environment::Air)
    }

    /// Instance ground environment by its flags — [`UnitType::is_ground`].
    #[inline]
    pub fn is_ground(&self) -> bool {
        self.environment() == Some(Environment::Ground)
    }

    /// Instance sea environment by its flags — [`UnitType::is_sea`].
    #[inline]
    pub fn is_sea(&self) -> bool {
        self.environment() == Some(Environment::Sea)
    }

    /// Instance land-sea environment — [`UnitType::is_amphibious`].
    #[inline]
    pub fn is_amphibious(&self) -> bool {
        self.environment() == Some(Environment::Amphibious)
    }

    /// Whether the instance is mobile (`STATIONARY` cleared) — [`UnitType::is_mobile`].
    #[inline]
    pub fn is_mobile(&self) -> bool {
        self.mobility() == Mobility::Mobile
    }

    /// Whether the instance is a building — [`UnitType::is_building`] (turrets are buildings).
    #[inline]
    pub fn is_building(&self) -> bool {
        self.mobility() == Mobility::Building
    }

    /// Whether the instance is stationary-other — [`UnitType::is_stationary`].
    #[inline]
    pub fn is_stationary(&self) -> bool {
        self.mobility() == Mobility::Stationary
    }

    /// Whether the instance is combat (by its `base_values`) — [`UnitType::is_combat`].
    #[inline]
    pub fn is_combat(&self) -> bool {
        self.combat_role().is_combat()
    }
}

// ---------------------------------------------------------------------------
// Tests: exhaustive verification of the 93 types against direct flag/attribute checks
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "classification_tests.rs"]
mod tests;
