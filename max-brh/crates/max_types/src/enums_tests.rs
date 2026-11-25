use super::*;
use crate::unit::UnitId;

fn roundtrip_u8<T>(variants: &[T])
where
    T: Copy + fmt::Debug + PartialEq + TryFrom<u8, Error = InvalidEnumValue>,
    u8: From<T>,
{
    for &variant in variants {
        let raw = u8::from(variant);
        assert_eq!(T::try_from(raw).unwrap(), variant, "u8 round-trip failed");
    }
}

fn roundtrip_u16<T>(variants: &[T])
where
    T: Copy + fmt::Debug + PartialEq + TryFrom<u16, Error = InvalidEnumValue>,
    u16: From<T>,
{
    for &variant in variants {
        let raw = u16::from(variant);
        assert_eq!(T::try_from(raw).unwrap(), variant, "u16 round-trip failed");
    }
}

#[test]
fn enum_counts_match_cpp() {
    assert_eq!(TeamKind::COUNT, 5);
    assert_eq!(TeamIndex::COUNT, 5);
    assert_eq!(TeamClan::COUNT, 9);
    assert_eq!(Opponent::COUNT, 6);
    assert_eq!(PlayMode::COUNT, 3);
    assert_eq!(VictoryType::COUNT, 2);
    assert_eq!(FileType::COUNT, 10);
    assert_eq!(Planet::COUNT, 24);
    assert_eq!(ResearchTopic::COUNT, 8);
    assert_eq!(Surface::ALL.bits(), 0x0F);
    assert_eq!(UnitType::COUNT, 93);
    assert_eq!(GameStateType::COUNT, 16);
    assert_eq!(WinLossState::COUNT, 4);
    assert_eq!(AiStrategy::COUNT, 10);
    assert_eq!(CautionLevel::COUNT, 4);
    assert_eq!(UnitOrder::COUNT, 32);
    assert_eq!(UnitOrderState::COUNT, 47);

    assert_eq!(UNIT_END, 93);
    assert_eq!(UNIT_TYPE_COUNT, 93);
    assert_eq!(ORDER_COUNT_MAX, 32);
    assert_eq!(RESEARCH_TOPIC_COUNT, 8);
    assert_eq!(AI_STRATEGY_MAX, 10);
    assert_eq!(PLAYER_TEAM_MAX, 5);
}

#[test]
fn key_values_match_cpp() {
    // resourcetable.hpp
    assert_eq!(UnitType::GoldRefinery as u16, 0);
    assert_eq!(UnitType::Tank as u16, 0x33);
    assert_eq!(UnitType::Tank as u16, 51);
    assert_eq!(UnitType::Harvester as u16, 0x5B);
    assert_eq!(UnitType::DeadWaldo as u16, 0x5C);
    assert_eq!(UnitType::DeadWaldo as u16, 92);
    // enums.hpp
    assert_eq!(UnitOrder::Await as u8, 0x00);
    assert_eq!(UnitOrder::Order17 as u8, 0x12);
    assert_eq!(UnitOrder::HaltBuilding2 as u8, 0x1F);
    assert_eq!(UnitOrder::MoveToAttack as u8, 0x1E);
    assert_eq!(UnitOrderState::Init as u8, 0x00);
    assert_eq!(UnitOrderState::BuildAbort as u8, 0x2E);
    assert_eq!(GameStateType::FatalError as u8, 0x0F);
    assert_eq!(GameStateType::InGame as u8, 0x08);
    assert_eq!(TeamKind::None as u8, 0);
    assert_eq!(TeamKind::Player as u8, 1);
    assert_eq!(TeamKind::Computer as u8, 2);
    assert_eq!(TeamKind::Remote as u8, 3);
    assert_eq!(TeamKind::Eliminated as u8, 4);
    assert_eq!(TeamClan::Random as u8, 0);
    assert_eq!(TeamClan::AxisInc as u8, 8);
    assert_eq!(Planet::Bottleneck as u8, 23);
    assert_eq!(FileType::MultiScenario as u8, 9);
    assert_eq!(ResearchTopic::Cost as u8, 7);
    // save.md
    assert_eq!(TeamIndex::Red as u8, 0);
    assert_eq!(TeamIndex::Alien as u8, 4);
}

#[test]
fn unit_type_tags_match_resources() {
    // The token is the first part of the doc comment of a variant (`/// TOKEN / C++_NAME`);
    // it is also the key of the UFN table in the presentation layer, so the 93 variants
    // have exactly 93 distinct tokens in the resource format.
    assert_eq!(UnitType::COUNT, 93);
    assert_eq!(UnitType::GoldRefinery.tag(), "COMMTWR");
    assert_eq!(UnitType::MaterialStorage.tag(), "ADUMP");
    assert_eq!(UnitType::FuelStorage.tag(), "FDUMP");
    assert_eq!(UnitType::ResearchCentre.tag(), "RESEARCH");
    assert_eq!(UnitType::EcoSphere.tag(), "GREENHSE");
    assert_eq!(UnitType::Artillery.tag(), "ARTYTRRT");
    assert_eq!(UnitType::MissileLauncher.tag(), "ANTIMSSL");
    assert_eq!(UnitType::ConcreteBlock.tag(), "BLOCK");
    assert_eq!(UnitType::ArmouredPersonnelCarrier.tag(), "CLNTRANS");
    assert_eq!(UnitType::Infiltrator.tag(), "COMMANDO");
    assert_eq!(UnitType::Escort.tag(), "FASTBOAT");
    assert_eq!(UnitType::Gunboat.tag(), "BATTLSHP");
    assert_eq!(UnitType::MissileCruiser.tag(), "MSSLBOAT");
    assert_eq!(UnitType::MiningStation.tag(), "MININGST");
    assert_eq!(UnitType::Harvester.tag(), "HARVSTER");
    assert_eq!(UnitType::DeadWaldo.tag(), "WALDO");

    let mut seen = std::collections::HashSet::new();
    for unit_type in UnitType::ALL {
        let tag = unit_type.tag();
        assert!(
            (4..=8).contains(&tag.len()),
                "{unit_type:?}: токен «{tag}» не в формате ресурса (4–8 символов)"
        );
        assert!(
            tag.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
                "{unit_type:?}: токен «{tag}» не в формате ресурса (A-Z, 0-9, _)"
        );
        assert!(seen.insert(tag), "повторяющийся токен «{tag}» ({unit_type:?})");
    }
    assert_eq!(seen.len(), UnitType::COUNT);
}

#[test]
fn all_u8_enums_roundtrip() {
    roundtrip_u8(TeamKind::ALL);
    roundtrip_u8(TeamIndex::ALL);
    roundtrip_u8(TeamClan::ALL);
    roundtrip_u8(Opponent::ALL);
    roundtrip_u8(PlayMode::ALL);
    roundtrip_u8(VictoryType::ALL);
    roundtrip_u8(FileType::ALL);
    roundtrip_u8(Planet::ALL);
    roundtrip_u8(ResearchTopic::ALL);
    roundtrip_u8(GameStateType::ALL);
    roundtrip_u8(WinLossState::ALL);
    roundtrip_u8(AiStrategy::ALL);
    roundtrip_u8(CautionLevel::ALL);
    roundtrip_u8(UnitOrder::ALL);
    roundtrip_u8(UnitOrderState::ALL);
    roundtrip_u8(UnitType::ALL);
}

#[test]
fn all_enums_roundtrip_u16() {
    roundtrip_u16(TeamKind::ALL);
    roundtrip_u16(TeamIndex::ALL);
    roundtrip_u16(TeamClan::ALL);
    roundtrip_u16(Opponent::ALL);
    roundtrip_u16(PlayMode::ALL);
    roundtrip_u16(VictoryType::ALL);
    roundtrip_u16(FileType::ALL);
    roundtrip_u16(Planet::ALL);
    roundtrip_u16(ResearchTopic::ALL);
    roundtrip_u16(GameStateType::ALL);
    roundtrip_u16(WinLossState::ALL);
    roundtrip_u16(AiStrategy::ALL);
    roundtrip_u16(CautionLevel::ALL);
    roundtrip_u16(UnitOrder::ALL);
    roundtrip_u16(UnitOrderState::ALL);
    roundtrip_u16(UnitType::ALL);
}

#[test]
fn invalid_values_are_rejected() {
    assert!(UnitType::try_from(93u16).is_err());
    assert!(UnitType::try_from(0x5Eu16).is_err());
    assert!(UnitType::try_from(u16::MAX).is_err());
    assert!(UnitOrder::try_from(32u8).is_err());
    assert!(UnitOrderState::try_from(0x2Fu8).is_err());
    assert!(GameStateType::try_from(16u8).is_err());
    assert!(TeamKind::try_from(5u8).is_err());
    assert!(TeamIndex::try_from(5u8).is_err());
    assert!(Planet::try_from(24u8).is_err());
    assert!(Surface::try_from(0x10u8).is_err());

    let err = UnitType::try_from(0x5Fu16).unwrap_err();
    assert_eq!(err, InvalidEnumValue::new("UnitType", 0x5F));
    assert!(err.to_string().contains("UnitType"));
}

#[test]
fn surface_is_a_bitmask() {
    let land_coast = Surface::LAND | Surface::COAST;
    assert_eq!(land_coast.bits(), 0x05);
    assert!(land_coast.contains(Surface::LAND));
    assert!(land_coast.contains(Surface::COAST));
    assert!(!land_coast.contains(Surface::WATER));
    assert!(land_coast.intersects(Surface::COAST | Surface::AIR));
    assert!(Surface::NONE.is_empty());
    assert_eq!((!Surface::LAND).bits(), 0x0E);
    assert_eq!(
        Surface::from_bits_truncate(0xFF).bits(),
        Surface::ALL.bits()
    );

    // All 16 valid bit combinations pass the round-trip.
    for bits in 0u8..=0x0F {
        let surface = Surface::from_bits(bits).unwrap();
        assert_eq!(Surface::try_from(surface.bits()).unwrap(), surface);
    }
    for bits in 0x10u8..=u8::MAX {
        assert!(Surface::from_bits(bits).is_none());
    }
}

#[test]
fn team_ids_and_indices() {
    assert_eq!(TeamId::RED, TeamId(0));
    assert_eq!(TeamId::GREEN, TeamId(1));
    assert_eq!(TeamId::BLUE, TeamId(2));
    assert_eq!(TeamId::GRAY, TeamId(3));
    assert_eq!(TeamId::ALIEN, TeamId(4));
    assert_eq!(TeamId::MAX, TeamId(5));

    for index in TeamIndex::ALL {
        let team: TeamId = (*index).into();
        assert_eq!(TeamIndex::try_from(team).unwrap(), *index);
    }
    assert!(TeamIndex::try_from(TeamId::MAX).is_err());
}

#[test]
fn unit_id_bitfield() {
    assert_eq!(UnitId::NO_ID.0, 0xFFFF);
    assert!(UnitId::NO_ID.is_no_id());
    assert!(!UnitId(0).is_no_id());

    let id = UnitId::new(TeamId::GREEN, 7);
    assert_eq!(id.0, (1 << 13) | 7);
    assert_eq!(id.team(), TeamId::GREEN);
    assert_eq!(id.object_index(), 7);

    // The index is masked with 13 bits, as in C++.
    let id = UnitId::new(TeamId::GRAY, 0xFFFF);
    assert_eq!(id.object_index(), 0x1FFF);
    assert_eq!(id.team(), TeamId::GRAY);
}

#[test]
fn byte_constants_match_cpp() {
    assert_eq!(unit_flags::GROUND_COVER, 0x1);
    assert_eq!(unit_flags::BUILDING, 0x10);
    assert_eq!(unit_flags::MOBILE_AIR_UNIT, 0x40);
    assert_eq!(unit_flags::ELECTRONIC_UNIT, 0x200000);
    assert_eq!(unit_flags::REGENERATING_UNIT, 0x10000000);

    assert_eq!(CARGO_FUEL, 0x20);
    assert_eq!(CARGO_GOLD, 0x40);
    assert_eq!(CARGO_MATERIALS, 0x80);
    assert_eq!(CARGO_MASK, 0x1F);

    assert_eq!(CONNECTOR_NORTH_LEFT, 0x1);
    assert_eq!(CONNECTOR_WEST_BOTTOM, 0x80);
    assert_eq!(CONNECTION_BEING_TESTED, 0x400);

    assert_eq!(AI_STATE_NO_RETREAT, 1);
    assert_eq!(AI_STATE_MOVE_FINISHED_REMINDER, 0x100);
    assert_eq!(AI_STATE_RETREAT_REMINDER, 0x200);
    assert_eq!(RESOURCE_MAP_VISIBILITY_MASK, 0x3C00);
}
