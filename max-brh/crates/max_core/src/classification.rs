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
mod tests {
    use super::*;
    use crate::access::{unit_type_flags, UNIT_TYPE_PROPERTIES};
    use crate::enums::unit_flags::HOVERING;
    use crate::team::TeamId;
    use crate::unit::UnitId;

    /// Direct `SC_UNITS` flag-bit check — independent of the classifier.
    fn has_flag(unit_type: UnitType, mask: u32) -> bool {
        UNIT_TYPE_PROPERTIES[unit_type as usize].flags & mask != 0
    }

    /// Direct `SC_ATTRI` attribute check.
    fn base_attack(unit_type: UnitType) -> u16 {
        base_unit_values(unit_type).attack
    }

    /// Expected form by flags and the turret list (from `object-classifier.md`).
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

    /// Expected footprint by the `BUILDING` flag (`object-classifier.md`).
    fn expected_footprint(unit_type: UnitType) -> Footprint {
        if has_flag(unit_type, BUILDING) {
            Footprint::Quad
        } else {
            Footprint::Single
        }
    }

    /// Expected environment by flags (`MOBILE_*`).
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

        // 25 combat types in `SC_ATTRI` (turrets, mines, mobile vehicles).
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

            // Data invariant: `BUILDING ⟹ STATIONARY` (`object-classifier.md`).
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

        // Turrets: the `BUILDING` flag is cleared, so only the semantic
        // exception (the owner's decision) makes them buildings.
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

        // 26 buildings by `Mobility`, of which 4 turrets have `BUILDING` cleared: 22 + 71.
        assert_eq!((quad, single), (22, 71));

        // Stable serialization tags — explicit discriminants, not order.
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

        // The converse is false: turrets are `Building` but `Single`.
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
        // 2×2 decorations carry `BUILDING` (`SC_UNITS`) — they are `Quad`.
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

        // 1×1 decorations — `Single`.
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

        // Instance: footprint by instance flags.
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
                // Air does not combine with land-sea flags — otherwise
                // the `Air` priority would hide an amphibian.
                assert!(
                    !has_flag(unit_type, MOBILE_SEA_UNIT | MOBILE_LAND_UNIT),
                    "{unit_type:?}: MOBILE_AIR_UNIT вместе с SEA/LAND"
                );
            }

            // The environment is defined only for mobile types.
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

        // Air=5, Amphibious=5, Ground=17, Sea=9, no environment=57.
        assert_eq!(counts, [57, 5, 5, 17, 9]);

        // Projectiles and trails are mobile by flag, but have no environment.
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
        // An independent list — exactly the `Cargo_GetNetProduction` branches
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
                // Producing buildings are non-combat (`object-classifier.md`).
                assert_eq!(
                    CombatRole::classify(unit_type),
                    CombatRole::NonCombat,
                    "{unit_type:?}"
                );
            }
        }
        assert_eq!(producing, 12);
        assert_eq!(PRODUCING_UNIT_TYPES.len(), 12);

        // `BARRACKS`/`SUPRTPLT` are not producing: they are absent from
        // `Cargo_GetNetProduction` (repair/alien building).
        assert!(!UnitType::Barracks.is_producing());
        assert!(!UnitType::SupportPlant.is_producing());
    }

    #[test]
    fn cargo_kind_matches_cargo_type_for_all_93_types() {
        // Independent lists by `CARGO_TYPE_*` groups (`unit.hpp`).
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

        // Grouping of `CARGO_TYPE_*` — all 7 C++ values.
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
        // Scout — amphibious combat.
        assert_eq!(Mobility::classify(UnitType::Scout), Mobility::Mobile);
        assert_eq!(
            Environment::classify(UnitType::Scout),
            Some(Environment::Amphibious)
        );
        assert_eq!(CombatRole::classify(UnitType::Scout), CombatRole::Combat);

        // Awac — air non-combat.
        assert_eq!(
            Environment::classify(UnitType::Awac),
            Some(Environment::Air)
        );
        assert_eq!(CombatRole::classify(UnitType::Awac), CombatRole::NonCombat);

        // GunTurret — a building by the owner's decision, although `BUILDING` is cleared.
        assert!(!has_flag(UnitType::GunTurret, BUILDING));
        assert_eq!(Mobility::classify(UnitType::GunTurret), Mobility::Building);
        assert_eq!(
            CombatRole::classify(UnitType::GunTurret),
            CombatRole::Combat
        );

        // LandMine — a stationary combat unit (not a building).
        assert_eq!(Mobility::classify(UnitType::LandMine), Mobility::Stationary);
        assert_eq!(CombatRole::classify(UnitType::LandMine), CombatRole::Combat);

        // Production: the gold refinery — yes, the barracks — no.
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
        // A ground tank.
        let mut tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId::NO_ID);
        tank.flags = unit_type_flags(UnitType::Tank);
        assert_eq!(tank.mobility(), Mobility::Mobile);
        assert_eq!(tank.environment(), Some(Environment::Ground));

        // Runtime: air flags instead of ground ones — the instance is classified
        // by its own flags, the type table does not participate.
        tank.flags = (tank.flags & !(MOBILE_SEA_UNIT | MOBILE_LAND_UNIT)) | MOBILE_AIR_UNIT;
        assert_eq!(tank.mobility(), Mobility::Mobile);
        assert_eq!(tank.environment(), Some(Environment::Air));

        // `HOVERING` is a flight state, not an environment.
        tank.flags |= HOVERING;
        assert_eq!(tank.environment(), Some(Environment::Air));

        // The building has `BUILDING`/`STATIONARY` dynamically cleared — the instance is
        // "mobile", although the type remains a building.
        let mut barracks = UnitInfo::new(UnitType::Barracks, TeamId::RED, UnitId::NO_ID);
        barracks.flags = unit_type_flags(UnitType::Barracks);
        assert_eq!(barracks.mobility(), Mobility::Building);
        barracks.flags &= !(BUILDING | STATIONARY);
        assert_eq!(barracks.mobility(), Mobility::Mobile);
        assert_eq!(barracks.environment(), None);
        assert_eq!(Mobility::classify(UnitType::Barracks), Mobility::Building);

        // Instance combat role — by its `base_values` (upgrades/clan).
        let mut tank = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId::NO_ID);
        assert_eq!(tank.combat_role(), CombatRole::Combat);
        tank.base_values.attack = 0;
        assert_eq!(tank.combat_role(), CombatRole::NonCombat);
        assert_eq!(CombatRole::classify(UnitType::Tank), CombatRole::Combat);

        // Production and cargo are type properties; instance flags do not change them.
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

            // Environments are mutually exclusive: at most one at a time.
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

        // The `STATIONARY` bit is equivalent to `!is_mobile()` if the
        // invariant `BUILDING ⟹ STATIONARY` holds.
        for &unit_type in UnitType::ALL {
            assert_eq!(
                !unit_type.is_mobile(),
                has_flag(unit_type, STATIONARY),
                "{unit_type:?}"
            );
        }

        // A turret is a building by the owner's decision, but its `BUILDING` bit is cleared:
        // these are different questions (`is_building()` ≠ `flags & BUILDING`).
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

        // Cleared `STATIONARY`/`BUILDING` on a building instance — the instance is
        // mobile, the type remains a building.
        let mut barracks = UnitInfo::new(UnitType::Barracks, TeamId::RED, UnitId::NO_ID);
        barracks.flags = unit_type_flags(UnitType::Barracks);
        assert!(barracks.is_building());
        assert!(!barracks.is_mobile());
        barracks.flags &= !(BUILDING | STATIONARY);
        assert!(!barracks.is_building());
        assert!(barracks.is_mobile());

        // Air flags instead of ground ones — the instance is "air".
        tank.flags = (tank.flags & !(MOBILE_SEA_UNIT | MOBILE_LAND_UNIT)) | MOBILE_AIR_UNIT;
        assert!(tank.is_air());
        assert!(!tank.is_ground());
    }

    // -----------------------------------------------------------------------
    // Object kind (F4): unit / effect / decoration / plot decoration
    // -----------------------------------------------------------------------

    /// Effect tokens — verbatim from `object-classifier.md`, "Non-units".
    const EFFECT_TOKENS: [&str; 14] = [
        "ROCKET", "TORPEDO", "ALNMISSL", "ALNTBALL", "ALNABALL", "LNDEXPLD", "AIREXPLD",
        "SEAEXPLD", "BLDEXPLD", "HITEXPLD", "LRGRUBLE", "SMLRUBLE", "RKTSMOKE", "TRPBUBLE",
    ];

    /// Decoration tokens — verbatim from `object-classifier.md`, "Non-units".
    const DECORATION_TOKENS: [&str; 6] = [
        "LRGTAPE", "SMLTAPE", "LRGSLAB", "SMLSLAB", "LRGCONES", "SMLCONES",
    ];

    /// Plot decoration tokens — verbatim from `object-classifier.md`.
    const PLOT_PROP_TOKENS: [&str; 3] = ["MASTER", "HARVSTER", "WALDO"];

    /// Independent oracle: the expected kind by resource token [`UnitType::tag`].
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

        // 70 units + 14 effects + 6 decorations + 3 plot props = 93 (a total
        // bijection, no gaps or duplicates).
        assert_eq!(counts, [70, 14, 6, 3]);
        assert_eq!(counts.iter().sum::<usize>(), UnitType::ALL.len());
        assert_eq!(counts.iter().sum::<usize>(), 93);

        // Exception lists — without duplicates and exactly the declared length.
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

        // Unit range boundaries by token: `COMMTWR`..`ALNPLANE`.
        assert_eq!(UnitType::GoldRefinery.tag(), "COMMTWR");
        assert_eq!(UnitType::AlienPlane.tag(), "ALNPLANE");
        assert_eq!(UnitType::GoldRefinery.object_kind(), ObjectKind::Unit);
        assert_eq!(UnitType::AlienPlane.object_kind(), ObjectKind::Unit);
    }

    #[test]
    fn object_kind_variants_are_mutually_exclusive() {
        // The kinds differ: `Unit` is not equal to the others.
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

        // Exactly one helper fires for each of the 93 types.
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
        // (c) Units: a producing building, the mine, a combat mobile.
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

        // Effects: explosion, trail, rubble.
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

        // Decoration: a 2×2 build site.
        assert_eq!(UnitType::LargeTape.object_kind(), ObjectKind::Decoration);
        assert!(UnitType::LargeTape.is_decoration());
        assert!(!UnitType::LargeTape.is_unit());

        // Plot decorations: `MASTER`, `HARVSTER`, `WALDO`.
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
        // Stable serialization tags — explicit discriminants, not order.
        assert_eq!(ObjectKind::Unit as u8, 0);
        assert_eq!(ObjectKind::Effect as u8, 1);
        assert_eq!(ObjectKind::Decoration as u8, 2);
        assert_eq!(ObjectKind::PlotProp as u8, 3);
    }
}
