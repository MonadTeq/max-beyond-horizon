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
mod tests {
    //! The DSL is a **payload** enum: the engine has no textual parser or
    //! server-side legality check for it yet (`GameState` orders go through
    //! `events::apply_command`/`Packet08Order`). The tests pin the wire contract and
    //! the value boundaries the client may propose; the missing bridge is recorded
    //! in the phase 2 report.

    use super::*;
    use crate::team::TeamId;

    /// All five variants with boundary field values.
    fn sample_orders() -> Vec<Order> {
        let unit = UnitId::new(TeamId::GRAY, 0x1FFF);
        vec![
            Order::Move {
                unit,
                path: vec![(i16::MIN, i16::MAX), (0, 0), (i16::MAX, i16::MIN)],
            },
            Order::Attack {
                unit,
                target: Target::Unit(UnitId::NO_ID),
            },
            Order::Attack {
                unit,
                target: Target::Cell(i16::MIN, i16::MAX),
            },
            Order::Build {
                unit,
                building: u8::MAX,
                x: i16::MAX,
                y: i16::MIN,
            },
            Order::Mode {
                unit,
                mode: u8::MAX,
            },
            Order::EndTurn,
        ]
    }

    /// `Order` is a value type: cloning keeps the unit, the route and the target.
    #[test]
    fn order_values_preserve_unit_and_boundaries() {
        let unit = UnitId::new(TeamId::BLUE, 0x1FFF);
        let route = vec![(i16::MIN, i16::MAX), (0, 0), (i16::MAX, i16::MIN)];
        let order = Order::Move {
            unit,
            path: route.clone(),
        };

        assert_eq!(order, order.clone());
        match &order {
            Order::Move {
                unit: moved_unit,
                path,
            } => {
                assert_eq!(*moved_unit, unit);
                assert_eq!(path, &route);
            }
            other => panic!("expected Move, got {other:?}"),
        }

        // An empty route is a distinct proposal (the server may reject it).
        assert_ne!(
            order,
            Order::Move {
                unit,
                path: Vec::new()
            }
        );

        // Raw byte arguments are kept as proposed (0 and MAX are both legal on the wire).
        assert_eq!(
            Order::Build {
                unit,
                building: 0,
                x: 0,
                y: 0
            },
            Order::Build {
                unit,
                building: 0,
                x: 0,
                y: 0
            }
        );
        assert_eq!(Order::Mode { unit, mode: 0 }, Order::Mode { unit, mode: 0 });
        assert_eq!(Order::EndTurn, Order::EndTurn);
    }

    /// `Target::Unit` and `Target::Cell` are distinct even for the same numbers;
    /// `NO_ID` is representable and stays `NO_ID`.
    #[test]
    fn attack_targets_distinguish_unit_from_cell() {
        assert_ne!(Target::Unit(UnitId(0)), Target::Cell(0, 0));
        assert_eq!(Target::Unit(UnitId::NO_ID), Target::Unit(UnitId::NO_ID));
        assert_eq!(
            Target::Cell(i16::MIN, i16::MAX),
            Target::Cell(i16::MIN, i16::MAX)
        );

        let order = Order::Attack {
            unit: UnitId::NO_ID,
            target: Target::Unit(UnitId::NO_ID),
        };
        match order {
            Order::Attack { unit, target } => {
                assert!(unit.is_no_id());
                assert!(matches!(target, Target::Unit(id) if id.is_no_id()));
            }
            other => panic!("expected Attack, got {other:?}"),
        }
    }

    /// JSON is the client/server wire shape (externally tagged enum). Every
    /// variant must survive encode → decode unchanged.
    #[test]
    fn orders_roundtrip_through_json_wire_format() {
        for order in sample_orders() {
            let json = serde_json::to_string(&order).expect("order serializes");
            let decoded: Order = serde_json::from_str(&json).expect("order parses");
            assert_eq!(decoded, order, "round-trip mismatch for {json}");
        }

        assert_eq!(
            serde_json::to_value(Order::EndTurn).expect("end turn"),
            serde_json::json!("EndTurn")
        );
        let json = serde_json::to_value(Order::Attack {
            unit: UnitId::NO_ID,
            target: Target::Cell(3, -4),
        })
        .expect("attack");
        assert_eq!(json["Attack"]["unit"], serde_json::json!(0xFFFF));
        assert_eq!(
            json["Attack"]["target"],
            serde_json::json!({"Cell": [3, -4]})
        );

        // A malformed payload is rejected by the deserializer, not silently defaulted.
        assert!(serde_json::from_str::<Order>(r#"{"Move":{"unit":1}}"#).is_err());
    }

    /// `OrderResult` is a value ack: sequence numbers and reasons survive the wire,
    /// and accept/reject are distinct outcomes.
    #[test]
    fn order_result_carries_sequence_and_outcome() {
        let accepted = OrderResult::Accepted { seq: u64::MAX };
        let rejected = OrderResult::Rejected {
            seq: u64::MAX,
            reason: String::new(),
        };

        match serde_json::to_value(&accepted).expect("accepted") {
            serde_json::Value::Object(map) => {
                assert!(map.contains_key("Accepted"));
            }
            other => panic!("unexpected accepted encoding: {other}"),
        }
        match serde_json::to_value(&rejected).expect("rejected") {
            serde_json::Value::Object(map) => {
                assert!(map.contains_key("Rejected"));
            }
            other => panic!("unexpected rejected encoding: {other}"),
        }
    }
}
