use crate::enums::{UnitOrder, UnitOrderState, UnitType};
use crate::geo::{Direction, Point, DIRECTION_OFFSETS, MAP_TILES};
use crate::rng::Randomizer;
use crate::{Team, TeamId, TeamKind};

#[test]
fn core_smoke() {
    let team = Team::new(TeamId(0), TeamKind::Player);
    assert_eq!(team.kind, TeamKind::Player);
}

#[test]
fn p0_constants_are_visible_from_crate_root() {
    assert_eq!(UnitType::COUNT, 93);
    assert_eq!(UnitOrder::COUNT, 32);
    assert_eq!(UnitOrderState::COUNT, 47);
    assert_eq!(MAP_TILES, 12_544);
    assert_eq!(Direction::COUNT, DIRECTION_OFFSETS.len());
    assert_eq!(Point::new(1, 2).x, 1);
}

#[test]
fn p1_rng_is_deterministic() {
    let mut first = Randomizer::new(0xDEAD_BEEF);
    let mut second = Randomizer::new(0xDEAD_BEEF);
    for _ in 0..1000 {
        assert_eq!(first.next_u32(), second.next_u32());
    }
}

#[test]
fn p3_types_are_visible_from_crate_root() {
    // Attribute table, domain commands and the save layer are available from the root.
    assert_eq!(crate::BASE_UNIT_VALUES.len(), crate::UNIT_COUNT);
    assert_eq!(crate::UNIT_COUNT, 93);
    let units = crate::TeamUnits::with_clan(crate::TeamClan::Musashi);
    assert_eq!(units.complexes().len(), 0);
    assert_eq!(
        crate::SaveFile::parse(SAVE_BYTES).map(|save| save.header.version),
        Ok(70)
    );
    assert_eq!(
        crate::SaveFile::parse(SAVE_BYTES)
            .expect("fixture parses")
            .units
            .len(),
        312
    );
}

const SAVE_BYTES: &[u8] = include_bytes!("../tests/data/save1_v70.cam");
