use super::*;

#[test]
fn map_size_matches_geometry() {
    assert_eq!(MAP_TILES, 12_544);
    assert_eq!(MapSize::STANDARD.tiles(), MAP_TILES);
    let map = Map::new();

    assert_eq!(map.size, crate::geo::MAP_SIZE);
    assert_eq!(map.surface.tiles().len(), MAP_TILES);
    assert_eq!(map.resources.tiles().len(), MAP_TILES);
}

#[test]
fn resource_entry_layout_matches_save_example() {
    // Example from the state-model.md §3.10 / save.md: 0x208C = 12 materials,
    // surveyed only by the reds. The reds are `HASH_TEAM_RED = 0x2000`
    // (bit 13), i.e. `visibility() == 0b1000` — that is how
    // `Survey_SurveyArea` (`survey.cpp:116`).
    let entry = ResourceMapEntry::new(12, Some(ResourceKind::Raw), 0b1000);

    assert_eq!(entry.bits(), 0x208C);
    assert_eq!(entry.amount(), 12);
    assert_eq!(entry.resource_type(), Some(ResourceKind::Raw));
    assert_eq!(entry.visibility(), 0b1000);
    assert!(entry.is_visible_to(0), "0x2000 — бит красных (команда 0)");
    assert!(!entry.is_visible_to(3));

    let decoded = ResourceMapEntry::from_bits(0x208C);
    assert_eq!(decoded, entry);
}

#[test]
fn resource_entry_bit_ranges() {
    for amount in 0..=0x0Fu16 {
        let entry = ResourceMapEntry::from_bits(amount);
        assert_eq!(entry.amount() as u16, amount);
        assert_eq!(entry.resource_type(), None);
        assert_eq!(entry.visibility(), 0);
    }

    for (bits, kind) in [
        (CARGO_MATERIALS as u16, ResourceKind::Raw),
        (CARGO_FUEL as u16, ResourceKind::Fuel),
        (CARGO_GOLD as u16, ResourceKind::Gold),
    ] {
        let entry = ResourceMapEntry::from_bits(bits | 0x000F);
        assert_eq!(entry.resource_type(), Some(kind));
        assert_eq!(entry.amount(), 0x0F);
    }

    // Two type bits at once — an invalid tile, the type is not decoded.
    let invalid = ResourceMapEntry::from_bits(CARGO_FUEL as u16 | CARGO_GOLD as u16);
    assert_eq!(invalid.resource_type(), None);

    for team in 0..=3u8 {
        let entry = ResourceMapEntry::from_bits(
            (team_visibility_bit(team) as u16) << TEAM_VISIBILITY_SHIFT,
        );
        assert_eq!(entry.visibility(), team_visibility_bit(team));
        assert_eq!(entry.resource_type(), None);
        assert!(entry.is_visible_to(team));

        // C++ `hash_team_id`: the reds are the highest bit of the field (13), the grays
        // the lowest (10).
        assert_eq!(
            entry.bits(),
            match team {
                0 => 0x2000,
                1 => 0x1000,
                2 => 0x0800,
                _ => 0x0400,
            },
                "бит команды {team} — HASH_TEAM_* из enums.hpp:40-43"
        );
    }

    // Knowledge is added without losing the amount/type and without duplicates.
    let entry = ResourceMapEntry::new(7, Some(ResourceKind::Gold), 0).with_team_visible(0);
    assert_eq!(entry.amount(), 7);
    assert_eq!(entry.resource_type(), Some(ResourceKind::Gold));
    assert!(entry.is_visible_to(0));
    assert!(!entry.is_visible_to(1));
    assert_eq!(
        entry.with_team_visible(0),
        entry,
            "повторная отметка идемпотентна"
    );
    assert!(entry.with_team_visible(1).is_visible_to(1));

    // Bits 14/15 are not part of the visibility mask.
    assert_eq!(ResourceMapEntry::from_bits(0xC000).visibility(), 0);
}

#[test]
fn resource_entry_with_methods_keep_other_fields() {
    let entry = ResourceMapEntry::new(5, Some(ResourceKind::Fuel), 0b0011);

    let gold = entry.with_kind(Some(ResourceKind::Gold));
    assert_eq!(gold.amount(), 5);
    assert_eq!(gold.visibility(), 0b0011);
    assert_eq!(gold.resource_type(), Some(ResourceKind::Gold));

    let empty = gold.with_kind(None);
    assert_eq!(empty.resource_type(), None);
    assert_eq!(empty.bits(), (5 | (0b0011 << TEAM_VISIBILITY_SHIFT)));

    let big = entry.with_amount(0xFF);
    assert_eq!(big.amount(), 0x1F);

    assert_eq!(entry.with_visibility(0xFF).visibility(), 0x0F);
}

#[test]
fn surface_map_indexing_roundtrip() {
    let mut surface = SurfaceMap::new();

    assert_eq!(surface.get(0, 0), Surface::NONE);
    assert!(surface.set(111, 111, Surface::LAND | Surface::COAST));
    assert_eq!(
        surface.get(111, 111),
        Surface::LAND | Surface::COAST,
            "tile bytes are a bitmask, not an exclusive enum"
    );
    assert_eq!(SurfaceMap::index(111, 111), Some(MAP_TILES - 1));
    assert_eq!(SurfaceMap::index(112, 0), None);
    assert_eq!(SurfaceMap::index(-1, 0), None);
    assert_eq!(SurfaceMap::index(0, 112), None);

    assert!(!surface.set(-1, 5, Surface::WATER));
    assert!(!surface.set(5, 112, Surface::WATER));
    assert_eq!(surface.get(-1, 5), Surface::NONE);
    assert_eq!(surface.get(5, 112), Surface::NONE);

    let bytes = surface.to_bytes();
    assert_eq!(bytes.len(), MAP_TILES);
    assert_eq!(bytes[MAP_TILES - 1], 0x05);
    assert_eq!(bytes[0], 0x00);
}

#[test]
fn map_accessors_use_size_invariant() {
    let mut map = Map::new();

    assert!(map.surface.set(3, 4, Surface::WATER));
    assert!(map
        .resources
        .set(3, 4, ResourceMapEntry::new(7, Some(ResourceKind::Gold), 0)));

    assert_eq!(map.surface_at(3, 4), Surface::WATER);
    assert_eq!(map.resource_at(3, 4).amount(), 7);
    assert_eq!(map.index(3, 4), Some(4 * 112 + 3));
    assert_eq!(map.surface_at(112, 0), Surface::NONE);
    assert_eq!(map.resource_at(0, 112), ResourceMapEntry::NONE);
}

#[test]
fn resource_map_counts_and_byte_layout() {
    let mut map = ResourceMap::new();

    map.set(0, 0, ResourceMapEntry::new(4, Some(ResourceKind::Raw), 0));
    map.set(1, 0, ResourceMapEntry::new(9, Some(ResourceKind::Fuel), 0));
    map.set(2, 0, ResourceMapEntry::new(2, Some(ResourceKind::Gold), 0));
    map.set(3, 0, ResourceMapEntry::new(0, None, 0xF));

    let counts = map.counts();

    assert_eq!(counts.raw_tiles, 1);
    assert_eq!(counts.raw_amount, 4);
    assert_eq!(counts.fuel_tiles, 1);
    assert_eq!(counts.fuel_amount, 9);
    assert_eq!(counts.gold_tiles, 1);
    assert_eq!(counts.gold_amount, 2);

    let bytes = map.to_le_bytes();
    assert_eq!(bytes.len(), MAP_TILES * 2);
    assert_eq!(&bytes[0..2], &0x0084u16.to_le_bytes());
    assert_eq!(&bytes[2..4], &0x0029u16.to_le_bytes());
    assert_eq!(&bytes[4..6], &0x0042u16.to_le_bytes());
    assert_eq!(&bytes[6..8], &0x3C00u16.to_le_bytes());
}

#[test]
fn serde_impls_are_available_for_map_types() {
    // Serialization is manual (arrays of 12544 elements are not supported
    // by the serde derive macro); we check that the impls are declared and usable
    // as a trait object.
    fn assert_serialize<T: Serialize>() {}
    fn assert_deserialize<T: for<'de> Deserialize<'de>>() {}

    assert_serialize::<Map>();
    assert_deserialize::<Map>();
    assert_serialize::<SurfaceMap>();
    assert_deserialize::<SurfaceMap>();
    assert_serialize::<ResourceMap>();
    assert_deserialize::<ResourceMap>();
    assert_serialize::<ResourceMapEntry>();
}
