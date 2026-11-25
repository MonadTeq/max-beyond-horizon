//! Helpers for the simulation-invariant suites
//! (`tests/invariants_economy_property.rs`, `tests/invariants_combat_property.rs`).
//!
//! The catalog (`max-brh-docs/development/simulation-invariants.md`) asks for
//! generated states as well as boundary values. `proptest` is not a dependency of
//! `max_core`; the suites use the small deterministic LCG below instead, so every
//! run explores the same cases and a regression has a reproducible seed.

use max_core::team::{ComplexId, TeamId};
use max_core::universe::GameUniverse;
use max_core::UnitId;

/// Deterministic 64-bit LCG (Knuth's MMIX constants).
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// A generator with a fixed seed; the suite fixes every seed explicitly.
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed.wrapping_mul(2).wrapping_add(1),
        }
    }

    /// Next pseudo-random word.
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 32) as u32
    }

    /// Uniform-looking integer in `lo..=hi` (empty range -> `lo`).
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        let span = (hi - lo + 1) as u32;
        lo + (self.next_u32() % span) as i32
    }

    /// One element of a non-empty slice.
    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        assert!(!items.is_empty(), "Lcg::pick needs a non-empty slice");
        items[(self.next_u32() as usize) % items.len()]
    }
}

/// The complex of a spawned building (`UnitInfo::GetComplex()`).
pub fn complex_of(universe: &GameUniverse, id: UnitId) -> ComplexId {
    universe
        .snapshot()
        .unit(id)
        .unwrap_or_else(|| panic!("unit {id:?} must exist"))
        .complex
        .unwrap_or_else(|| panic!("unit {id:?} must be a complex member"))
}

/// Writes the order pair directly (no path side effects) for pure net-production
/// checks; the oracle `Cargo_GetNetProduction` reads exactly these two fields.
pub fn set_order_pair(
    universe: &mut GameUniverse,
    id: UnitId,
    order: max_core::UnitOrder,
    state: max_core::UnitOrderState,
) {
    let unit = universe
        .unit_mut(id)
        .unwrap_or_else(|| panic!("unit {id:?} must exist"));
    unit.orders = order;
    unit.state = state;
}

/// The stock of a complex as cached by `Access_UpdateResourcesTotal`.
pub fn complex_stock(
    universe: &GameUniverse,
    team: TeamId,
    complex: ComplexId,
) -> max_core::team::Complex {
    *universe
        .snapshot()
        .complex(team, complex.0 as u16)
        .unwrap_or_else(|| panic!("complex {complex:?} of {team:?} must exist"))
}
