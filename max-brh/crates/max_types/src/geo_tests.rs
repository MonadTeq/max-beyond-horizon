use super::*;

#[test]
fn map_dimensions_match_original() {
    assert_eq!(MAP_WIDTH, 112);
    assert_eq!(MAP_HEIGHT, 112);
    assert_eq!(MAP_TILES, 12544);
    assert_eq!(MAP_SIZE.width, 112);
    assert_eq!(MAP_SIZE.height, 112);
}

#[test]
fn point_index_roundtrip() {
    for index in [0usize, 1, 111, 112, 113, 12543] {
        let point = Point::from_index(index);
        assert_eq!(point.index(), index);
    }
    assert_eq!(Point::new(0, 0).index(), 0);
    assert_eq!(Point::new(111, 111).index(), 12543);
}

#[test]
fn point_arithmetic_uses_i32_then_narrows() {
    assert_eq!(Point::new(10, 20) + Point::new(3, -5), Point::new(13, 15));
    assert_eq!(Point::new(10, 20) - Point::new(3, -5), Point::new(7, 25));

    let mut p = Point::new(1, 2);
    p += Point::new(2, 2);
    p -= Point::new(1, 1);
    assert_eq!(p, Point::new(2, 3));

    // i16 overflow narrowing — like `as short` in C++.
    assert_eq!(
        Point::new(i16::MAX, 0) + Point::new(1, 0),
        Point::new(i16::MIN, 0)
    );
}

#[test]
fn point_distances() {
    let a = Point::new(0, 0);
    let b = Point::new(3, 4);
    assert_eq!(a.manhattan_distance(b), 7);
    assert_eq!(a.chebyshev_distance(b), 4);
    assert_eq!(a.distance_squared(b), 25);
    assert_eq!(a.euclidean_distance(b), 5);
    assert_eq!(a.euclidean_distance(a), 0);
    assert_eq!(a.euclidean_distance(Point::new(1, 1)), 1); // sqrt(2) is truncated
}

#[test]
fn direction_values_match_cpp() {
    assert_eq!(Direction::North as u8, 0);
    assert_eq!(Direction::NorthEast as u8, 1);
    assert_eq!(Direction::East as u8, 2);
    assert_eq!(Direction::SouthEast as u8, 3);
    assert_eq!(Direction::South as u8, 4);
    assert_eq!(Direction::SouthWest as u8, 5);
    assert_eq!(Direction::West as u8, 6);
    assert_eq!(Direction::NorthWest as u8, 7);
    assert_eq!(Direction::COUNT, DIRECTION_OFFSETS.len());
}

#[test]
fn direction_offsets_match_cpp() {
    let expected = [
        (0, -1),
        (1, -1),
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
    ];
    for (direction, (x, y)) in Direction::ALL.iter().zip(expected) {
        assert_eq!(direction.offset(), Point::new(x, y));
    }
    assert_eq!(
        DIRECTION_OFFSETS,
        [
            Point::new(0, -1),
            Point::new(1, -1),
            Point::new(1, 0),
            Point::new(1, 1),
            Point::new(0, 1),
            Point::new(-1, 1),
            Point::new(-1, 0),
            Point::new(-1, -1),
        ]
    );
}

#[test]
fn direction_roundtrip_and_rotation() {
    for direction in Direction::ALL {
        let raw = u8::from(direction);
        assert_eq!(Direction::try_from(raw).unwrap(), direction);
        assert_eq!(Direction::try_from(raw as u16).unwrap(), direction);
        assert_eq!(direction.opposite().opposite(), direction);
        assert_eq!(direction.rotate_cw().rotate_ccw(), direction);
        assert_eq!(Direction::from_raw_masked(raw), direction);
        assert_eq!(Direction::from_raw_masked(raw + 8), direction);
    }
    assert_eq!(Direction::North.opposite(), Direction::South);
    assert_eq!(Direction::NorthEast.rotate_ccw(), Direction::North);
    assert!(Direction::try_from(8u8).is_err());
    assert!(Direction::try_from(0x100u16).is_err());
}

#[test]
fn step_and_direction_to() {
    let origin = Point::new(10, 10);
    assert_eq!(origin.step(Direction::North), Point::new(10, 9));
    assert_eq!(origin.step(Direction::SouthEast), Point::new(11, 11));
    assert_eq!(
        origin.step(Direction::West).direction_to(origin),
        Some(Direction::East)
    );
    assert_eq!(origin.direction_to(origin), None);
    // Not a neighbor — the direction is undefined.
    assert_eq!(origin.direction_to(Point::new(12, 10)), Some(Direction::East));
    assert_eq!(origin.direction_to(Point::new(11, 10)), Some(Direction::East));
}

#[test]
fn path_step_matches_cpp_types() {
    let step = PathStep::new(-1, 1);
    assert_eq!(step.as_point(), Point::new(-1, 1));
    assert_eq!(core::mem::size_of::<PathStep>(), 2);
    assert_eq!(core::mem::size_of::<Point>(), 4);
}
