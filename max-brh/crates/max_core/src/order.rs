//! Orders — client input that is sent to the server.

use serde::{Deserialize, Serialize};

use crate::unit::UnitId;

/// An order to a unit. The client only *proposes* an order; the server decides legality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Order {
    /// Move to a point (route).
    Move { unit: UnitId, path: Vec<(i16, i16)> },
    /// Attack a unit/point.
    Attack { unit: UnitId, target: Target },
    /// Build a building at a point.
    Build {
        unit: UnitId,
        building: u8,
        x: i16,
        y: i16,
    },
    /// Change mode (fire/patrol/...).
    Mode { unit: UnitId, mode: u8 },
    /// End of turn.
    EndTurn,
}

/// Attack target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Target {
    Unit(UnitId),
    Cell(i16, i16),
}

/// Confirmation/rejection of an order by the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderResult {
    Accepted { seq: u64 },
    Rejected { seq: u64, reason: String },
}

#[cfg(test)]
#[path = "order_tests.rs"]
mod tests;
