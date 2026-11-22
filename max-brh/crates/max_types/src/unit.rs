//! Unit identifiers and presentation data (Infrastructure).
//!
//! Contains [`UnitId`] (the original's 16-bit id, high bits are the team),
//! [`UnitListKind`] — a leaf type without `max_core` data, [`Rect`] —
//! a rectangle in pixels, and [`UnitPresentation`] — presentation fields
//! of the unit (UI/render) that do not participate in the simulation but live in the same
//! V70 object and are therefore needed by `max_core::save` for a byte-exact round-trip.
//!
//! The unit itself (`UnitInfo`), its paths (`GroundPath`/`AirPath`/`BuilderPath`)
//! stay in `max_core::unit`: they depend on attributes and frame data.
//!
//! Oracle: `max-cpp-port/src/unitinfo.hpp` (`UnitInfo::hash_id`),
//! `units_manager.cpp` (five unit lists).

use serde::{Deserialize, Serialize};

use crate::geo::Point;
use crate::team::TeamId;

/// Unit identifier (the original's 16-bit id).
///
/// High bits are the team, the low 13 bits are the object index within the team
/// (`(team << 13) | object_index`), see DESIGN.md §3.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UnitId(pub u16);

impl UnitId {
    /// `NO_ID` (0xFFFF) — absence of a unit.
    pub const NO_ID: Self = Self(0xFFFF);

    /// Builds an id from the team and the object index.
    #[inline]
    pub const fn new(team: TeamId, object_index: u16) -> Self {
        Self(((team.0 as u16) << 13) | (object_index & 0x1FFF))
    }

    /// Team from the high bits of the id.
    #[inline]
    pub const fn team(self) -> TeamId {
        TeamId((self.0 >> 13) as u8)
    }

    /// Object index within the team (low 13 bits).
    #[inline]
    pub const fn object_index(self) -> u16 {
        self.0 & 0x1FFF
    }

    /// Whether the id is "empty" (`NO_ID`).
    #[inline]
    pub const fn is_no_id(self) -> bool {
        self.0 == Self::NO_ID.0
    }

    /// From `u16` with V70 semantics: 0xFFFF = [`UnitId::NO_ID`].
    #[inline]
    pub const fn from_v70(raw: u16) -> Self {
        Self(raw)
    }

    /// Into `u16` for writing to a V70 file.
    #[inline]
    pub const fn to_v70(self) -> u16 {
        self.0
    }
}

/// One of the five unit lists (`UnitInfo::unit_list`, `units_manager.cpp`).
///
/// The order of variants is the traversal order of `UnitsManager_ProcessOrders`
/// (DESIGN §5.2): ground cover → mobile land/sea → stationary → mobile air →
/// particles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum UnitListKind {
    /// Ground cover (`UnitsManager_GroundCoverUnits`).
    GroundCover = 0,
    /// Mobile land and sea (`UnitsManager_MobileLandSeaUnits`).
    MobileLandSea = 1,
    /// Stationary (`UnitsManager_StationaryUnits`).
    Stationary = 2,
    /// Mobile air (`UnitsManager_MobileAirUnits`).
    MobileAir = 3,
    /// Particles/missiles/projectiles (`UnitsManager_ParticleUnits`).
    Particle = 4,
}

impl UnitListKind {
    /// All five lists in canonical traversal order.
    pub const ALL: [Self; 5] = [
        Self::GroundCover,
        Self::MobileLandSea,
        Self::Stationary,
        Self::MobileAir,
        Self::Particle,
    ];

    /// List index (0..5).
    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// List by index.
    #[inline]
    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::GroundCover),
            1 => Some(Self::MobileLandSea),
            2 => Some(Self::Stationary),
            3 => Some(Self::MobileAir),
            4 => Some(Self::Particle),
            _ => None,
        }
    }
}

/// Rectangle in pixels — C++ `struct Rect` (`point.hpp`/`gnw.h`).
///
/// Not involved in the simulation: a cache of sprite dimensions (UI), saved in V70.
/// Lives in the lower layer (moved from `max_core::unit`, §5.3), so
/// [`UnitPresentation`] can reference it without an upward dependency.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    /// Top-left X.
    pub ulx: i32,
    /// Top-left Y.
    pub uly: i32,
    /// Bottom-right X.
    pub lrx: i32,
    /// Bottom-right Y.
    pub lry: i32,
}

/// Presentation fields of the unit — C++ `class UnitInfo`, the UI/V70 subset
/// (`state-model.md` §6.4; §5.3, part B).
///
/// The fields do not participate in the simulation, the snapshot, `replay_digest` or the packet 45 CRC:
/// they are read only by `max_core::save` (byte-exact V70 round-trip) and
/// by the client renderer (`max_app`). Verification of each field is the §5.3/B0 report.
///
/// Kept in parallel with `UnitInfo` slots in `max_core::game::GameState`
/// (presentation is not part of `UnitInfo`, so that the application layer does not carry
/// UI data). No logic — only a constructor.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitPresentation {
    /// Shadow offset (UI/V70).
    pub shadow_offset: Point,
    /// Brightness (UI/V70: damage visual).
    pub brightness: u8,
    /// Sound effect (C++ `Unit::SfxType`, u8) — UI/V70.
    pub sound: u8,
    /// Cache of sprite dimensions (UI/V70).
    pub sprite_bounds: Rect,
    /// Cache of shadow dimensions (UI/V70).
    pub shadow_bounds: Rect,
    /// Turret offset X (UI/V70).
    pub turret_offset_x: i8,
    /// Turret offset Y (UI/V70).
    pub turret_offset_y: i8,
    /// Total frames (UI/V70).
    pub total_images: i16,
    /// Fire sprite base (UI/V70).
    pub firing_image_base: i16,
    /// Connector sprite base (UI/V70).
    pub connector_image_base: i16,
    /// Cursor over the unit (UI/V70).
    pub cursor: u8,
    /// Bobbing on water (UI/V70).
    pub bobbed: bool,
}

impl UnitPresentation {
    /// Initial presentation state — C++ `UnitInfo::UnitInfo`
    /// (`unitinfo.cpp:276-294`): `brightness = 255`, the remaining fields are zero.
    pub fn new() -> Self {
        Self {
            shadow_offset: Point::ZERO,
            brightness: u8::MAX,
            sound: 0,
            sprite_bounds: Rect::default(),
            shadow_bounds: Rect::default(),
            turret_offset_x: 0,
            turret_offset_y: 0,
            total_images: 0,
            firing_image_base: 0,
            connector_image_base: 0,
            cursor: 0,
            bobbed: false,
        }
    }
}

impl Default for UnitPresentation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C++ `UnitInfo::UnitInfo` (`unitinfo.cpp:276-294`): `brightness = 255`,
    /// the remaining presentation fields are zero.
    #[test]
    fn presentation_defaults_match_cpp_unit_constructor() {
        let presentation = UnitPresentation::default();
        assert_eq!(presentation.brightness, u8::MAX);
        assert_eq!(presentation.shadow_offset, Point::ZERO);
        assert_eq!(presentation.sound, 0);
        assert_eq!(presentation.sprite_bounds, Rect::default());
        assert_eq!(presentation.shadow_bounds, Rect::default());
        assert_eq!(presentation.total_images, 0);
        assert_eq!(presentation.cursor, 0);
        assert!(!presentation.bobbed);
        assert_eq!(presentation, UnitPresentation::new());
    }
}
