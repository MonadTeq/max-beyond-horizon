//! max_types — common domain and protocol types of M.A.X. (Infrastructure).
//!
//! The crate was carved out in Phase 5.2 (owner's decision, option "b": `PLAN-refactor.md`
//! §3.3) as a shared lower layer on which `max_core` (Application),
//! `max_net` (Infrastructure) and `max_assets` (Infrastructure) depend. This removes
//! the formal violation of rule 5 (`max_assets → max_core`,
//! `max_net → max_core`) for **pure** types.
//!
//! ## What lives here
//!
//! * [`enums`] — all C++ enumerations/bit masks with exact `#[repr]`,
//!   discriminants and `TryFrom` conversions; `UnitType` (93 types);
//! * [`geo`] — `Point`, `PathStep`, `Direction`, `DIRECTION_OFFSETS`, map
//!   dimensions;
//! * [`map`] — `SurfaceMap`, `ResourceMapEntry`, `ResourceMap`, `Map`;
//! * [`team`] — leaf team types: `TeamId`, `TeamKind`, `Team`,
//!   `ResearchTopicInfo`, `Complex`, `Cargo`;
//! * [`unit`] — `UnitId` (16-bit id with the team in the high bits) and
//!   `UnitListKind`;
//! * [`input`] — protocol DTOs of incoming packets (`Command` and payloads);
//! * [`events`] — DTOs of the authoritative team state (packet 54);
//! * [`victory`] — `MissionCategory` (mission category).
//!
//! ## What must not be here
//!
//! Types that read `max_core` data (`UnitValues`, the
//! `UNIT_TYPE_PROPERTIES` table, `TeamInfo`/`TeamUnits`, `UnitInfo`) stay in
//! `max_core`; `UnitType` methods that read that data live in the extension trait
//! `max_core::UnitTypeClassify` (sealed), so that the orphan rule holds.
//!
//! Relocation principle (rule 2): the types were moved **verbatim** — `#[repr]`,
//! discriminants, field order, derives and bit values were not changed, so
//! CRC/determinism/V70 are unaffected.

#![forbid(unsafe_code)]
#![deny(clippy::float_arithmetic)]

pub mod enums;
pub mod events;
pub mod geo;
pub mod input;
pub mod map;
pub mod team;
pub mod unit;
pub mod victory;

/// Number of game unit types (`UNIT_END` from `resourcetable.hpp`, 93).
///
/// It lives in the lower layer because it is the size of the domain arrays per type
/// (`UnitValues[UNIT_COUNT]`, team counters, upgrade packets) — needed by
/// `max_core`, `max_net` and `max_assets`.
pub const UNIT_COUNT: usize = enums::UNIT_END as usize;
