use super::*;

#[test]
fn unit_hash_buckets_and_order() {
    let mut hash = UnitHash::with_default_size();
    assert_eq!(hash.size(), HASH_UNIT_HASH_SIZE);
    let first = UnitId::new(crate::team::TeamId::RED, 5);
    let second = UnitId::new(crate::team::TeamId::GREEN, 5);
    hash.push_back(first);
    hash.push_back(second);
    // Bucket key is `id % hash_size`; both teams give 5 (`GREEN << 13` is a multiple of 8192),
    // so both units sit in the same bucket in insertion order.
    assert_eq!(hash.bucket((first.0 % 8192) as u16).unwrap(), &vec![first, second]);
    assert_eq!(hash.get(first), Some(first));
    assert_eq!(hash.get(second), Some(second));
    assert!(hash.remove(first));
    assert_eq!(hash.get(first), None);
    assert!(!hash.remove(first));
    hash.clear();
    assert_eq!(hash.iter_values().count(), 0);
}

#[test]
fn map_hash_sizing_matches_cpp() {
    let hash = MapHash::<UnitId>::for_standard_map();
    // C++ `SetMapSize` for 112×112: 512*8 < 12544 -> 1024 -> 2048 (2048*8 > 12544).
    assert_eq!(hash.size(), 4 * HASH_MAP_HASH_SIZE_MIN);
    // 2048 > 128 -> shifts: 1024, 512, 256, 128 => x_shift = 4.
    assert_eq!(hash.x_shift(), 4);
    assert_eq!(hash.bucket_index(0, 0), 0);
    assert_eq!(
        hash.bucket_index(3, 7),
        ((7u16 ^ (3 << 4)) % hash.size())
    );
    let big = MapHash::<UnitId>::set_map_size(Point::new(1000, 1000));
    assert_eq!(big.size(), HASH_MAP_HASH_SIZE_MAX);
    // The size from the V70 file (512) gives shift 2 — as in a real save.
    let saved = MapHash::<UnitId>::new(HASH_MAP_HASH_SIZE_MIN);
    assert_eq!(saved.x_shift(), 2);
}

#[test]
fn map_hash_adds_buildings_to_four_cells() {
    let mut hash = MapHash::<UnitId>::for_standard_map();
    let id = UnitId::new(crate::team::TeamId::RED, 9);
    // A mobile unit — one cell.
    hash.add(id, 10, 10, Footprint::Single, false);
    assert_eq!(hash.get(10, 10), Some(&vec![id]));
    assert_eq!(hash.get(11, 10), None);
    hash.remove(id, 10, 10, Footprint::Single);
    assert_eq!(hash.get(10, 10), None);

    // A building occupies 2×2.
    hash.add(id, 20, 20, Footprint::Quad, false);
    for (x, y) in [(20u16, 20u16), (21, 20), (20, 21), (21, 21)] {
        assert_eq!(hash.get(x, y), Some(&vec![id]), "cell {x},{y}");
    }
    hash.remove(id, 20, 20, Footprint::Quad);
    for (x, y) in [(20u16, 20u16), (21, 20), (20, 21), (21, 21)] {
        assert_eq!(hash.get(x, y), None, "cell {x},{y}");
    }
    // An empty cell object is deleted (C++ `list->Remove(object)`).
    assert_eq!(hash.iter_objects().count(), 0);
}

#[test]
fn map_hash_places_turret_in_single_cell() {
    use crate::classification::UnitTypeClassify;
    use crate::enums::UnitType;

    // A turret is `Mobility::Building` but `Footprint::Single`: in `MapHash` it
    // occupies exactly one cell (not 2×2), as in C++ (the `BUILDING` flag is cleared).
    let turret = UnitType::GunTurret;
    assert_eq!(turret.footprint(), Footprint::Single);
    assert!(turret.is_building());

    let mut hash = MapHash::<UnitId>::for_standard_map();
    let id = UnitId::new(crate::team::TeamId::RED, 10);
    hash.add(id, 30, 30, turret.footprint(), false);
    assert_eq!(hash.get(30, 30), Some(&vec![id]));
    for (x, y) in [(31u16, 30u16), (30, 31), (31, 31)] {
        assert_eq!(hash.get(x, y), None, "cell {x},{y} не должна заниматься");
    }
    hash.remove(id, 30, 30, turret.footprint());
    assert_eq!(hash.get(30, 30), None);
}

#[test]
fn map_hash_keeps_cpp_cell_order() {
    let mut hash = MapHash::<UnitId>::for_standard_map();
    let a = UnitId::new(crate::team::TeamId::RED, 1);
    let b = UnitId::new(crate::team::TeamId::RED, 2);
    // `mode == false` is C++ `!mode` — `PushFront`: the newest unit first.
    hash.add(a, 5, 5, Footprint::Single, false);
    hash.add(b, 5, 5, Footprint::Single, false);
    assert_eq!(hash.get(5, 5), Some(&vec![b, a]));
    // `mode == true` — `PushBack`.
    let mut hash = MapHash::<UnitId>::for_standard_map();
    hash.add(a, 5, 5, Footprint::Single, true);
    hash.add(b, 5, 5, Footprint::Single, true);
    assert_eq!(hash.get(5, 5), Some(&vec![a, b]));
    assert_eq!(
        hash.bucket_index(5, 5),
        ((5u16 ^ (5 << hash.x_shift())) % hash.size())
    );
}

#[test]
fn iterators_are_deterministic() {
    let mut hash = UnitHash::with_default_size();
    let ids: Vec<UnitId> = (0..10u16)
        .map(|index| UnitId::new(crate::team::TeamId::BLUE, index))
        .collect();
    for &id in &ids {
        hash.push_back(id);
    }
    let first: Vec<UnitId> = hash.iter_values().copied().collect();
    let second: Vec<UnitId> = hash.iter_values().copied().collect();
    assert_eq!(first, second);
    assert_eq!(first.len(), ids.len());
}

/// `MapHash::Init` boundary sizes: the shift changes exactly above 128.
#[test]
fn x_shift_boundaries() {
    assert_eq!(MapHash::<UnitId>::new(1).x_shift(), 0);
    assert_eq!(MapHash::<UnitId>::new(128).x_shift(), 0);
    assert_eq!(MapHash::<UnitId>::new(129).x_shift(), 1);
    assert_eq!(MapHash::<UnitId>::new(255).x_shift(), 1);
    assert_eq!(MapHash::<UnitId>::new(256).x_shift(), 1);
    assert_eq!(MapHash::<UnitId>::new(257).x_shift(), 1);
    assert_eq!(MapHash::<UnitId>::new(258).x_shift(), 2);
    assert_eq!(MapHash::<UnitId>::new(512).x_shift(), 2);
    assert_eq!(MapHash::<UnitId>::new(1024).x_shift(), 3);
    assert_eq!(MapHash::<UnitId>::new(2048).x_shift(), 4);
    assert_eq!(MapHash::<UnitId>::new(32768).x_shift(), 8);
}

/// `MapHash::SetMapSize` — the exact "8 cells per bucket" boundary.
#[test]
fn set_map_size_boundary_cells() {
    // 64×64 = 4096 = 512*8: the `<` keeps 512; one more cell doubles it.
    assert_eq!(MapHash::<UnitId>::set_map_size(Point::new(64, 64)).size(), 512);
    assert_eq!(MapHash::<UnitId>::set_map_size(Point::new(65, 65)).size(), 1024);
    // The maximum size is reached and not exceeded.
    let huge = MapHash::<UnitId>::set_map_size(Point::new(i16::MAX, i16::MAX));
    assert_eq!(huge.size(), HASH_MAP_HASH_SIZE_MAX);
    assert_eq!(huge.x_shift(), 8);
}

/// The bucket key is `key % size`; `u32::MAX` and `NO_ID` are valid keys.
#[test]
fn unit_hash_key_modulo_boundaries() {
    let mut hash = UnitHash::<u16>::new(512);
    hash.push_at(u32::MAX, 1);
    // `NO_ID` (0xFFFF) and `u32::MAX` both reduce to bucket 511.
    hash.push_at(u32::from(UnitId::NO_ID.0), 2);
    hash.push_at(0, 3);
    assert_eq!(hash.bucket(511), Some(&vec![1, 2]));
    assert_eq!(hash.bucket(0), Some(&vec![3]));
    assert_eq!(u32::MAX % 512, 511);
    assert_eq!(u32::from(UnitId::NO_ID.0) % 512, 511);
    // A table of size 1 puts every key into bucket 0.
    let mut single = UnitHash::<u16>::new(1);
    for key in [0u32, 1, 511, u32::MAX] {
        single.push_at(key, key as u16);
    }
    assert_eq!(single.bucket(0).map(Vec::len), Some(4));
}

/// `(y ^ (x << x_shift)) % size` with the `uint16_t` truncation of the shift.
#[test]
fn bucket_index_shift_truncation() {
    let hash = MapHash::<UnitId>::new(512); // x_shift = 2
    assert_eq!(hash.x_shift(), 2);
    // 65535 << 2 truncates to 0xFFFC; XOR 0xFFFF = 3.
    assert_eq!(hash.bucket_index(65535, 65535), 3);
    assert_eq!(hash.bucket_index(0, 0), 0);
    assert_eq!(hash.bucket_index(32, 0), 128);
    assert_eq!(hash.bucket_index(16, 0), 64);
    assert_eq!(hash.bucket_index(127, 0), 508);
    // 128 << 2 == 512 -> wraps to bucket 0.
    assert_eq!(hash.bucket_index(128, 0), 0);
}
