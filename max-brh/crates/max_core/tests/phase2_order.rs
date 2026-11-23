//! Phase 2 integration: `order.rs` — the client order DSL as a wire payload.
//!
//! Oracle: `max-cpp-port/src/` has no counterpart for this payload — the engine
//! takes orders as packet 08 (`Packet08Order`/`OrderPayload`). `order::Order` is
//! the client-side proposal type ("the server decides legality"), so this file
//! checks the wire contract (JSON round-trip), the value boundaries and the
//! "proposal does not mutate the simulation" invariant.
//!
//! Coverage gap (reported to the analyst/architect): there is no parser or
//! server-side validator for `Order`, and no bridge from `Order` to
//! `Packet08Order`/`GameUniverse`. A scenario "client order → server applies it"
//! cannot be built without inventing that mapping.

mod support;

use max_core::enums::UnitType;
use max_core::geo::Point;
use max_core::order::{Order, OrderResult, Target};
use max_core::team::TeamId;
use max_core::UnitId;

use support::{game, spawn};

fn sample_orders() -> Vec<Order> {
    vec![
        Order::Move {
            unit: UnitId::new(TeamId::RED, 0x1FFF),
            path: vec![(0, 0), (1, -1), (i16::MAX, i16::MIN)],
        },
        Order::Attack {
            unit: UnitId(0),
            target: Target::Unit(UnitId::NO_ID),
        },
        Order::Attack {
            unit: UnitId(0),
            target: Target::Cell(i16::MIN, i16::MAX),
        },
        Order::Build {
            unit: UnitId(0x3FFF),
            building: u8::MAX,
            x: -1,
            y: -1,
        },
        Order::Mode {
            unit: UnitId(0x3FFF),
            mode: 0,
        },
        Order::EndTurn,
    ]
}

/// Every client order survives JSON encode → decode unchanged (the wire shape is
/// the externally tagged enum).
#[test]
fn client_orders_roundtrip_over_the_wire() {
    for order in sample_orders() {
        let json = serde_json::to_string(&order).expect("order encodes");
        let decoded: Order = serde_json::from_str(&json).expect("order decodes");
        assert_eq!(decoded, order, "round-trip mismatch: {json}");
    }

    // Unknown/malformed payloads are rejected, not silently accepted.
    for malformed in [
        r#"{"Move":{"unit":1}}"#,
        r#"{"Attack":{"unit":1,"target":{"Bogus":1}}}"#,
        r#"{"Build":{"unit":1,"building":2}}"#,
        r#"42"#,
    ] {
        assert!(
            serde_json::from_str::<Order>(malformed).is_err(),
            "accepted malformed order {malformed}"
        );
    }
}

/// Command boundaries: ids, raw bytes and coordinates are preserved exactly.
#[test]
fn order_arguments_keep_their_boundaries() {
    let move_order = Order::Move {
        unit: UnitId::NO_ID,
        path: vec![(i16::MIN, i16::MIN), (i16::MAX, i16::MAX)],
    };
    let json = serde_json::to_value(&move_order).expect("move");
    assert_eq!(json["Move"]["unit"], serde_json::json!(0xFFFF));
    assert_eq!(
        json["Move"]["path"],
        serde_json::json!([[-32768, -32768], [32767, 32767]])
    );

    let build = Order::Build {
        unit: UnitId(0),
        building: u8::MAX,
        x: i16::MIN,
        y: i16::MAX,
    };
    let decoded: Order = serde_json::from_value(serde_json::to_value(&build).unwrap()).unwrap();
    assert_eq!(decoded, build);

    // `Target::Unit` and `Target::Cell` stay distinguishable on the wire.
    assert_ne!(
        serde_json::to_value(Target::Unit(UnitId(3))).unwrap(),
        serde_json::to_value(Target::Cell(0, 3)).unwrap()
    );
}

/// A client proposal carries unit ids that refer to the real simulation; the
/// payload alone changes nothing — the state digest before and after encoding,
/// decoding and dropping the order is identical.
#[test]
fn client_proposals_do_not_mutate_the_game_state() {
    let mut state = game();
    let red = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(5, 5));
    let green = spawn(&mut state, UnitType::Tank, TeamId::GREEN, Point::new(9, 5));

    let orders = [
        Order::Move {
            unit: red,
            path: vec![(1, 0), (1, 0)],
        },
        Order::Attack {
            unit: red,
            target: Target::Unit(green),
        },
        Order::Build {
            unit: red,
            building: UnitType::PowerGenerator as u8,
            x: 6,
            y: 6,
        },
        Order::Mode { unit: red, mode: 3 },
        Order::EndTurn,
    ];

    let before = state.snapshot().state_checksum();
    let before_snapshot = state.snapshot().snapshot_bytes();

    for order in orders {
        let json = serde_json::to_string(&order).expect("encode");
        let decoded: Order = serde_json::from_str(&json).expect("decode");
        assert_eq!(decoded, order);
    }

    assert_eq!(before, state.snapshot().state_checksum());
    assert_eq!(before_snapshot, state.snapshot().snapshot_bytes());
}

/// Server acknowledgements keep the sequence number and the rejection reason.
#[test]
fn order_acks_keep_sequence_and_reason() {
    let accepted = OrderResult::Accepted { seq: u64::MAX };
    let rejected = OrderResult::Rejected {
        seq: 7,
        reason: "unit is not selectable".to_owned(),
    };

    let accepted_json = serde_json::to_value(&accepted).expect("accepted");
    assert_eq!(
        accepted_json["Accepted"]["seq"],
        serde_json::json!(u64::MAX)
    );
    let rejected_json = serde_json::to_value(&rejected).expect("rejected");
    assert_eq!(rejected_json["Rejected"]["seq"], serde_json::json!(7));
    assert_eq!(
        rejected_json["Rejected"]["reason"],
        serde_json::json!("unit is not selectable")
    );
}
