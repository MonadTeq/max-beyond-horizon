//! Phase 4 — ordered hash tables (`hash.rs`) parity with `hash.cpp` (I + D).
//!
//! The port's contract is "same buckets, same order as C++", because the hash
//! iteration order reaches the wire (`Access_IsAccessible` walks a cell list)
//! and the V70 file (`UnitHash`/`MapHash` buckets). The tests pin:
//!
//! * the size constants (`HASH_UNIT_HASH_SIZE`/`_MAP_HASH_SIZE_MIN/MAX`);
//! * `MapHash::Init` (`x_shift`) and `MapHash::SetMapSize` against an
//!   independent transcription of the C++ formulas;
//! * the bucket functions (`id % hash_size`, `(y ^ (x << x_shift)) % size`);
//! * the ordering rules (insertion order within a bucket, `PushFront` of a new
//!   cell object, empty object removal, 2×2 `BUILDING` cells);
//! * the real V70 tables of `SAVE1.CAM`/`SAVE2.CAM`: every entry sits in the
//!   bucket its key dictates (a load-order lock, D layer).
//!
//! Oracle: `hash.cpp` (`Init`, `SetMapSize`, `AddEx`, `RemoveEx`,
//! `FileLoad`/`FileSave`), `hash.hpp`.

use max_core::classification::Footprint;
use max_core::geo::Point;
use max_core::hash::{
    MapHash, UnitHash, HASH_MAP_HASH_SIZE_MAX, HASH_MAP_HASH_SIZE_MIN, HASH_UNIT_HASH_SIZE,
};
use max_core::save::SaveFile;
use max_core::team::TeamId;
use max_core::UnitId;

/// `max-original/SAVE1.CAM` — `UnitHash`/`MapHash` with 512 buckets.
const SAVE1: &[u8] = include_bytes!("data/save1_v70.cam");
/// `max-original/SAVE2.CAM` — the same table sizes, a different unit set.
const SAVE2: &[u8] = include_bytes!("data/save2_v70.cam");

/// `MapHash::Init` (`hash.cpp:126`): `x_shift` grows while the size exceeds 128.
fn cpp_x_shift(size: u16) -> u16 {
    let mut shift = 0u16;
    let mut reduced = size;
    while reduced > 128 {
        shift += 1;
        reduced >>= 1;
    }
    shift
}

/// `MapHash::SetMapSize` (`hash.cpp:141`): about 8 map cells per bucket.
fn cpp_set_map_size(width: i16, height: i16) -> u16 {
    // C++ multiplies the two `int16_t` coordinates in `int`, so a negative size
    // still yields a positive cell count; the port clamps to 0. For every
    // representable map size the resulting bucket count is the same.
    let cells = i32::from(width) * i32::from(height);
    let mut size = HASH_MAP_HASH_SIZE_MIN;
    while size < HASH_MAP_HASH_SIZE_MAX && i32::from(size) * 8 < cells {
        size <<= 1;
    }
    size
}

#[test]
fn size_constants_match_cpp() {
    // `hash.cpp:26-28`.
    assert_eq!(HASH_UNIT_HASH_SIZE, 8192);
    assert_eq!(HASH_MAP_HASH_SIZE_MIN, 512);
    assert_eq!(HASH_MAP_HASH_SIZE_MAX, 32768);
    // The port's V70 writer uses 512 buckets as well (`save_write.rs`).
    assert_eq!(max_core::save_write::SAVE_UNIT_HASH_SIZE, 512);
}

#[test]
fn x_shift_matches_cpp_init() {
    for size in [
        1u16, 2, 64, 127, 128, 129, 130, 255, 256, 257, 511, 512, 513, 1024, 2047, 2048, 4096,
        8192, 16384, 32768,
    ] {
        let hash = MapHash::<UnitId>::new(size);
        assert_eq!(hash.size(), size);
        assert_eq!(hash.x_shift(), cpp_x_shift(size), "x_shift for size {size}");
        if size == 1 || size == 2 {
            // A tiny table is legal for the API but never used by the game.
            assert_eq!(hash.buckets().len(), size as usize);
        }
    }
}

#[test]
fn set_map_size_matches_cpp() {
    for (width, height) in [
        (-1i16, -1i16),
        (0, 0),
        (1, 1),
        (8, 8),
        (32, 32),
        (63, 64),
        (64, 64),
        (65, 65),
        (112, 112),
        (128, 128),
        (200, 200),
        (500, 500),
        (1000, 1000),
        (i16::MAX, i16::MAX),
    ] {
        let hash = MapHash::<UnitId>::set_map_size(Point::new(width, height));
        let expected = cpp_set_map_size(width, height);
        assert_eq!(hash.size(), expected, "size for {width}x{height}");
        assert_eq!(hash.x_shift(), cpp_x_shift(expected));
        assert_eq!(hash.buckets().len(), expected as usize);
    }

    // The stock 112×112 map: 512*8 < 12544 -> 1024 -> 2048 (2048*8 = 16384).
    let standard = MapHash::<UnitId>::for_standard_map();
    assert_eq!(standard.size(), 2048);
    assert_eq!(standard.x_shift(), 4);
}

#[test]
fn bucket_index_matches_cpp_formula() {
    for size in [512u16, 1024, 2048, 4096, 32768] {
        let hash = MapHash::<UnitId>::new(size);
        let shift = cpp_x_shift(size);
        for (x, y) in [
            (0u16, 0u16),
            (1, 0),
            (0, 1),
            (1, 1),
            (3, 7),
            (16, 5),
            (111, 111),
            (127, 128),
            (128, 127),
            (255, 255),
            (1000, 2000),
            (8191, 8191),
            (65_535, 65_535),
        ] {
            let expected = (u32::from(y) ^ (u32::from(x) << shift)) % u32::from(size);
            assert_eq!(
                u32::from(hash.bucket_index(x, y)),
                expected,
                "size {size}, cell ({x},{y})"
            );
            // `bucket()` and `bucket_index()` must agree.
            assert_eq!(hash.bucket(x, y).len(), 0);
        }
    }
}

#[test]
fn unit_hash_buckets_are_id_modulo_size() {
    let mut hash = UnitHash::<UnitId>::new(512);
    let mut ids = Vec::new();
    for team in [
        TeamId::RED,
        TeamId::GREEN,
        TeamId::BLUE,
        TeamId::GRAY,
        TeamId::ALIEN,
    ] {
        for index in [0u16, 1, 2, 511, 512, 513, 1000] {
            let id = UnitId::new(team, index);
            ids.push(id);
            hash.push_back(id);
        }
    }

    for id in &ids {
        let bucket = u32::from(id.0) % 512;
        assert!(
            hash.bucket(bucket as u16)
                .expect("bucket in range")
                .contains(id),
            "{id:?} is not in bucket {bucket}"
        );
        assert_eq!(hash.get(*id), Some(*id));
    }

    // Total iteration preserves bucket order, then insertion order inside a
    // bucket — the C++ `SmartList` traversal.
    let iterated: Vec<UnitId> = hash.iter_values().copied().collect();
    assert_eq!(iterated.len(), ids.len());
    let mut expected = ids.clone();
    expected.sort_by_key(|id| u32::from(id.0) % 512);
    assert_eq!(iterated, expected, "bucket order or bucket contents differ");
}

#[test]
fn unit_hash_collisions_keep_insertion_order_and_remove_first_occurrence() {
    // Size 1: every id lands in bucket 0.
    let mut hash = UnitHash::<UnitId>::new(1);
    let first = UnitId::new(TeamId::RED, 7);
    let second = UnitId::new(TeamId::GREEN, 7);
    let third = UnitId::new(TeamId::BLUE, 7);
    for id in [first, second, third] {
        hash.push_back(id);
    }
    assert_eq!(hash.bucket(0).expect("bucket"), &vec![first, second, third]);

    // A duplicate push is a duplicate entry; `remove` drops only the first.
    hash.push_back(first);
    assert_eq!(
        hash.bucket(0).expect("bucket"),
        &vec![first, second, third, first]
    );
    assert!(hash.remove(first));
    assert_eq!(hash.bucket(0).expect("bucket"), &vec![second, third, first]);
    assert!(hash.remove(first));
    assert!(!hash.remove(first));
    assert_eq!(hash.bucket(0).expect("bucket"), &vec![second, third]);
    assert_eq!(hash.iter_values().count(), 2);

    // `get` matches the exact id, not merely the bucket.
    assert_eq!(hash.get(second), Some(second));
    assert_eq!(hash.get(UnitId::new(TeamId::GRAY, 7)), None);
    assert_eq!(hash.get(UnitId::NO_ID), None);
}

#[test]
fn map_hash_cell_objects_keep_insertion_order_and_drop_empty() {
    let mut hash = MapHash::<UnitId>::for_standard_map();
    let first = UnitId::new(TeamId::RED, 1);
    let second = UnitId::new(TeamId::RED, 2);

    // Two cells sharing a bucket: (0,5) and (1,21) both hash to index 5.
    hash.add(first, 0, 5, Footprint::Single, false);
    hash.add(second, 1, 21, Footprint::Single, false);
    assert_eq!(hash.bucket_index(0, 5), hash.bucket_index(1, 21));
    let bucket = hash
        .buckets()
        .get(hash.bucket_index(0, 5) as usize)
        .expect("bucket");
    // A new cell object is pushed to the FRONT of the bucket (`PushFront` in
    // `MapHash::AddEx`), so the most recently created cell comes first.
    assert_eq!(bucket.len(), 2);
    assert!(bucket[0].matches(1, 21), "newest cell object must be first");
    assert!(bucket[1].matches(0, 5));

    // Units on the same cell keep insertion order (mode = false -> push_back).
    hash.add(second, 0, 5, Footprint::Single, false);
    assert_eq!(hash.get(0, 5), Some(&vec![first, second]));

    // Removing one unit keeps the cell, removing the last deletes the object.
    hash.remove(first, 0, 5, Footprint::Single);
    assert_eq!(hash.get(0, 5), Some(&vec![second]));
    hash.remove(second, 0, 5, Footprint::Single);
    assert_eq!(hash.get(0, 5), None);
    assert_eq!(hash.iter_objects().count(), 1, "cell (1,21) remains");
    assert_eq!(hash.get(1, 21), Some(&vec![second]));

    // `mode = true` pushes to the front of the cell list.
    let mut hash = MapHash::<UnitId>::for_standard_map();
    hash.add(first, 5, 5, Footprint::Single, true);
    hash.add(second, 5, 5, Footprint::Single, true);
    assert_eq!(hash.get(5, 5), Some(&vec![second, first]));

    hash.clear();
    assert_eq!(hash.iter_objects().count(), 0);
    assert_eq!(hash.get(5, 5), None);
}

#[test]
fn map_hash_adds_a_building_to_four_cells_and_wraps_at_the_edge() {
    let mut hash = MapHash::<UnitId>::for_standard_map();
    let building = UnitId::new(TeamId::RED, 3);

    // `BUILDING` occupies (x,y), (x+1,y), (x,y+1), (x+1,y+1).
    hash.add(building, 40, 50, Footprint::Quad, false);
    for (x, y) in [(40u16, 50u16), (41, 50), (40, 51), (41, 51)] {
        assert_eq!(hash.get(x, y), Some(&vec![building]), "cell ({x},{y})");
    }
    hash.remove(building, 40, 50, Footprint::Quad);
    for (x, y) in [(40u16, 50u16), (41, 50), (40, 51), (41, 51)] {
        assert_eq!(hash.get(x, y), None, "cell ({x},{y})");
    }
    assert_eq!(hash.iter_objects().count(), 0);

    // `grid_x + 1` wraps as `uint16_t` in C++; the port must not panic and must
    // not lose the removal symmetry.
    hash.add(building, u16::MAX, u16::MAX, Footprint::Quad, false);
    for (x, y) in [(u16::MAX, u16::MAX), (0, u16::MAX), (u16::MAX, 0), (0, 0)] {
        assert_eq!(
            hash.get(x, y),
            Some(&vec![building]),
            "wrapped cell ({x},{y})"
        );
    }
    hash.remove(building, u16::MAX, u16::MAX, Footprint::Quad);
    assert_eq!(hash.iter_objects().count(), 0);
}

#[test]
fn real_save_tables_are_consistent_with_their_keys() {
    for (source, tag) in [(SAVE1, "SAVE1"), (SAVE2, "SAVE2")] {
        let save = SaveFile::parse(source).expect("reference save parses");
        let object_to_id: std::collections::BTreeMap<u16, UnitId> = save
            .units
            .iter()
            .map(|unit| (unit.object_index, unit.info.id))
            .collect();

        // `UnitHash`: entry bucket == id % hash_size (`UnitHash::PushBack`).
        assert_eq!(save.unit_hash.size(), 512, "{tag}");
        for (bucket_index, bucket) in save.unit_hash.buckets().iter().enumerate() {
            for object_index in bucket {
                let id = object_to_id
                    .get(object_index)
                    .unwrap_or_else(|| panic!("{tag}: unknown object {object_index}"));
                assert_eq!(
                    u32::from(id.0) % u32::from(save.unit_hash.size()),
                    bucket_index as u32,
                    "{tag}: {id:?} in the wrong UnitHash bucket"
                );
            }
        }

        // `MapHash`: cell bucket == (y ^ (x << x_shift)) % hash_size.
        assert_eq!(save.map_hash.size(), 512, "{tag}");
        assert_eq!(save.map_hash.x_shift(), 2, "{tag}");
        let size = u32::from(save.map_hash.size());
        let shift = save.map_hash.x_shift();
        for (bucket_index, bucket) in save.map_hash.buckets().iter().enumerate() {
            for object in bucket {
                let expected = (u32::from(object.y) ^ (u32::from(object.x) << shift)) % size;
                assert_eq!(
                    expected, bucket_index as u32,
                    "{tag}: cell ({},{}) in the wrong MapHash bucket",
                    object.x, object.y
                );
                assert!(
                    !object.units.is_empty(),
                    "{tag}: empty cell object in the file"
                );
            }
        }

        // Every unit referenced by the cell table exists, and every unit in the
        // file sits at its own cell (the 2×2 rule was applied on load).
        for object in save.map_hash.iter_objects() {
            for object_index in &object.units {
                let id = object_to_id
                    .get(object_index)
                    .unwrap_or_else(|| panic!("{tag}: unknown object {object_index}"));
                let unit = save.unit_by_id(*id).expect("unit exists");
                let (dx, dy) = (
                    (object.x as i32) - (unit.info.grid.x as i32),
                    (object.y as i32) - (unit.info.grid.y as i32),
                );
                let single = unit.info.footprint() == Footprint::Single;
                let allowed = if single {
                    (dx, dy) == (0, 0)
                } else {
                    (0..=1).contains(&dx) && (0..=1).contains(&dy)
                };
                assert!(allowed, "{tag}: {id:?} in cell ({},{})", object.x, object.y);
            }
        }
    }
}

#[test]
fn standard_map_hash_places_every_unit_in_its_cell_object() {
    // The live `GameState` table is sized for the stock map (2048 buckets) — a
    // different size than the file (512), as C++ rehashes on load
    // (`hash.cpp:FileLoad`).
    let state = max_core::GameState::from_save(&SaveFile::parse(SAVE1).expect("parses"));
    assert_eq!(state.map_hash.size(), 2048);

    let mut placed = 0usize;
    for object in state.map_hash.iter_objects() {
        assert!(
            !object.units.is_empty(),
            "empty cell object in the live table"
        );
        placed += object.units.len();
    }
    assert!(
        placed >= state.unit_count(),
        "every unit must appear in at least its own cell"
    );
    for id in state.unit_ids_in_order() {
        let unit = state.unit(id).expect("unit");
        assert!(
            state.units_at(unit.grid).contains(&id),
            "{id:?} is missing from the cell table"
        );
    }
}
