use super::*;

fn p1(order: UnitOrder) -> PayloadP1 {
    PayloadP1 {
        order,
        state: UnitOrderState::Init,
        prior_order: UnitOrder::Await,
        prior_state: UnitOrderState::ExecutingOrder,
        disabled_reaction_fire: false,
    }
}

fn p2(order: UnitOrder) -> PayloadP2 {
    PayloadP2 {
        p1: p1(order),
        parent_unit: None,
        experience: 0,
    }
}

fn p3(order: UnitOrder) -> PayloadP3 {
    PayloadP3 {
        p2: p2(order),
        move_to: Point::new(1, 2),
        fire_on: Point::new(3, 4),
        enemy_unit: None,
        transfer_cargo: 0,
        stealth_dice_roll: 0,
    }
}

#[test]
fn processor_levels_match_cpp_init_table() {
    use UnitOrder::*;
    // Exact copy of the switch from Remote_Init (remote.cpp:661-724).
    assert_eq!(PayloadLevel::for_order(Order17), PayloadLevel::Empty);
    for order in [
        PowerOn,
        PowerOff,
        Explode,
        Sentry,
        Land,
        TakeOff,
        Load,
        Idle,
        HaltBuilding,
        AwaitScaling,
        AwaitTapePositioning,
        LayMine,
        HaltBuilding2,
    ] {
        assert_eq!(PayloadLevel::for_order(order), PayloadLevel::P1, "{order:?}");
    }
    for order in [Transform, Build, Clear, Disable] {
        assert_eq!(PayloadLevel::for_order(order), PayloadLevel::P4, "{order:?}");
    }
    for order in [
        Await,
        Move,
        Fire,
        Activate,
        Transfer,
        AwaitStealUnit,
        AwaitDisableUnit,
        MoveToUnit,
        MoveToAttack,
    ] {
        assert_eq!(PayloadLevel::for_order(order), PayloadLevel::P3, "{order:?}");
    }
    assert_eq!(
        PayloadLevel::for_order(NewAllocate),
        PayloadLevel::P5
    );
    for order in [Unload, Repair, Reload, Upgrade] {
        assert_eq!(PayloadLevel::for_order(order), PayloadLevel::P2, "{order:?}");
    }
    // All 32 orders are distributed (no forgotten variants).
    for order in UnitOrder::ALL {
        let _ = PayloadLevel::for_order(*order);
    }
}

#[test]
fn payload_nesting_exposes_p1_fields() {
    let payload = OrderPayload::P4(PayloadP4 {
        p3: p3(UnitOrder::Build),
        repeat_build: true,
        build_time: 3,
        build_rate: 2,
        build_list: vec![UnitType::Tank],
    });
    assert_eq!(payload.level(), PayloadLevel::P4);
    assert_eq!(payload.order(), Some(UnitOrder::Build));
    assert!(payload.matches_order(UnitOrder::Build));
    assert!(!payload.matches_order(UnitOrder::Move));

    let empty = OrderPayload::Empty;
    assert_eq!(empty.level(), PayloadLevel::Empty);
    assert_eq!(empty.order(), None);
    assert!(!empty.matches_order(UnitOrder::Order17));
}

#[test]
fn command_kinds_match_protocol_numbers() {
    let stop = Command::Stop(Packet50StopUnit {
        unit: crate::unit::UnitId(1),
    });
    assert_eq!(stop.kind(), PacketKind::StopUnit);
    assert_eq!(PacketKind::StopUnit as u8, 0x32);
    assert!(stop.affects_simulation());

    let chat = Command::Chat(Packet18Chat {
        sender: TeamId(0),
        message: "hello".into(),
    });
    assert!(!chat.affects_simulation());
    assert_eq!(PacketKind::Chat as u8, 0x12);

    // Packet 09 is a *simulation* command: gold and the research allocation
    // are part of the team state (`remote.cpp:2067-2107`).
    let team_state = Command::TeamState(Packet09TeamState {
        team: TeamId(0),
        gold: 1,
        gold_spent_on_upgrades: 2,
        research: [ResearchTopicInfo::default(); crate::enums::RESEARCH_TOPIC_COUNT],
        name: "RED".into(),
    });
    assert_eq!(team_state.kind(), PacketKind::TeamState);
    assert_eq!(PacketKind::TeamState as u8, 0x09);
    assert!(team_state.affects_simulation());
}

#[test]
fn team_and_order_validation() {
    assert!(is_valid_team_index(TeamId(0)));
    assert!(is_valid_team_index(TeamId(3)));
    assert!(!is_valid_team_index(TeamId(4)));
    assert!(!is_valid_team_index(TeamId(5)));
    assert!(is_valid_order(UnitOrder::HaltBuilding2));
    assert_eq!(PLAY_MODE_TURN_BASED, PlayMode::TurnBased);
    assert_eq!(PLAY_MODE_SIMULTANEOUS_MOVES, PlayMode::SimultaneousMoves);
}
