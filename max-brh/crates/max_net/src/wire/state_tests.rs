use super::*;
use max_core::universe::GameUniverse;
use max_types::enums::{UnitOrder, UnitOrderState, UnitType};
use max_types::geo::Point;
use max_types::team::TeamKind;
use max_types::UNIT_COUNT;

fn state_with_green_units(count: usize) -> (GameUniverse, Vec<UnitId>) {
    let mut state = GameUniverse::default();
    state
        .team_mut(TeamId::GREEN)
        .expect("GREEN slot")
        .team_type = TeamKind::Remote;
    *state.team_units_mut(TeamId::GREEN).expect("GREEN economy") =
        max_core::team::TeamUnits::with_clan(TeamClan::Random);

    let mut ids = Vec::with_capacity(count);
    for index in 0..count {
        let id = state
            .spawn_unit(
                UnitType::Tank,
                TeamId::GREEN,
                Point::new(10 + (index % 80) as i16, 10 + (index / 80) as i16),
                0,
            )
            .expect("tank");
        ids.push(id);
    }
    // A unit of another team must not end up in GREEN's 54.
    state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(50, 50), 0)
        .expect("red tank");
    (state, ids)
}

#[test]
fn state_sync_chunks_fit_transport_and_put_the_tail_last() {
    let (mut state, ids) = state_with_green_units(40);
    state
        .team_units_mut(TeamId::GREEN)
        .expect("GREEN economy")
        .gold = 777;
    state
        .team_mut(TeamId::GREEN)
        .expect("GREEN slot")
        .stats_gold_spent_on_upgrades = 12;
    state
        .team_mut(TeamId::GREEN)
        .expect("GREEN slot")
        .number_of_objects_created = 5;
    state
        .team_mut(TeamId::GREEN)
        .expect("GREEN slot")
        .team_unit_counters[UnitType::Tank as usize] = 9;
    state
        .team_mut(TeamId::GREEN)
        .expect("GREEN slot")
        .research_topics[2] = ResearchTopicInfo {
        research_level: 4,
        turns_to_complete: 5,
        allocation: 6,
    };
    state.set_order(ids[39], UnitOrder::Sentry, UnitOrderState::ExecutingOrder);

    let packets = state_sync_for(state.snapshot(), TeamId::GREEN, 3);
    assert_eq!(packets.len(), 2, "40 юнитов → 30 + 10");
    assert_eq!(STATE_SYNC_UNITS_PER_PACKET, 32, "номинал C++");

    let first = &packets[0];
    assert_eq!(first.team, TeamId::GREEN);
    assert_eq!(first.sync_index, 3);
    assert!(!first.is_last);
    assert_eq!(first.units.len(), 30);
    assert!(first.tail.is_none());
    // `MobileLandSea` uses `PushFront`, so the order is the reverse of spawn order.
    assert_eq!(first.units[0].0, ids[39]);
    assert_eq!(first.units[0].1.orders, UnitOrder::Sentry as u8);
    assert_eq!(first.units[0].1.team, TeamId::GREEN.0);
    assert_eq!(first.units[29].0, ids[10]);

    let last = &packets[1];
    assert!(last.is_last);
    assert_eq!(last.units.len(), 10);
    assert_eq!(last.units[0].0, ids[9]);
    assert_eq!(last.units[9].0, ids[0]);
    let tail = last.tail.as_ref().expect("tail on last chunk");
    assert_eq!(tail.gold, 777);
    assert_eq!(tail.gold_spent_on_upgrades, 12);
    assert_eq!(tail.number_of_objects_created, 5);
    assert_eq!(tail.unit_counters[UnitType::Tank as usize], 9);
    assert_eq!(
        tail.research[2],
        ResearchTopicWire {
            research_level: 4,
            turns_to_complete: 5,
            allocation: 6,
        }
    );
    assert_eq!(tail.unit_counters.len(), UNIT_COUNT);
}

#[test]
fn state_sync_for_an_empty_team_is_one_last_chunk() {
    let mut state = GameUniverse::default();
    state
        .team_mut(TeamId::BLUE)
        .expect("BLUE slot")
        .team_type = TeamKind::Remote;

    let packets = state_sync_for(state.snapshot(), TeamId::BLUE, 1);
    assert_eq!(packets.len(), 1);
    assert!(packets[0].is_last);
    assert!(packets[0].units.is_empty());
    assert!(packets[0].tail.is_some());

    // Round-trip: an empty last chunk encodes and decodes without loss.
    let bytes = super::super::Packet::StateSync(packets[0].clone())
        .encode()
        .expect("encode empty 54");
    assert_eq!(
        super::super::Packet::decode(&bytes).expect("decode empty 54"),
        super::super::Packet::StateSync(packets[0].clone())
    );
}

#[test]
fn state_sync_packets_fit_the_transport_limit() {
    // 100 units: non-final chunks of 30, the last one carries the 478-byte tail.
    let (state, _) = state_with_green_units(100);
    let packets = state_sync_for(state.snapshot(), TeamId::GREEN, 1);

    assert!(packets.len() >= 4);
    for packet in packets {
        let frame = super::super::Packet::StateSync(packet)
            .encode()
            .expect("каждый 54 обязан влезать в MAX_PACKET_SIZE");
        assert!(
            frame.len() <= super::super::MAX_PACKET_SIZE,
                "54 длиной {} > {}",
            frame.len(),
            super::super::MAX_PACKET_SIZE
        );
    }
}

#[test]
fn chunk_sizes_keep_the_tail_chunk_small() {
    assert_eq!(chunk_sizes(0), vec![0]);
    assert_eq!(chunk_sizes(20), vec![20]);
    assert_eq!(chunk_sizes(21), vec![1, 20]);
    assert_eq!(chunk_sizes(25), vec![5, 20]);
    assert_eq!(chunk_sizes(30), vec![10, 20]);
    assert_eq!(chunk_sizes(40), vec![30, 10]);
    assert_eq!(chunk_sizes(55), vec![30, 5, 20]);
    assert_eq!(chunk_sizes(60), vec![30, 10, 20]);
    assert_eq!(chunk_sizes(100), vec![30, 30, 30, 10]);
}

#[test]
fn wire_unit_state_converts_to_core_packet() {
    let (state, ids) = state_with_green_units(1);
    let unit = state.snapshot().unit(ids[0]).expect("unit");
    let mut wire = UnitState::from_unit(state.snapshot(), unit);
    wire.experience = 17;
    wire.storage = -3;

    let core = wire.to_core();
    assert_eq!(core.experience, 17);
    assert_eq!(core.storage, -3);
    assert_eq!(core.unit_type, UnitType::Tank as u16);
    assert_eq!(core.grid_y, 10);
}
