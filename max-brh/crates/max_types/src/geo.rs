//! Integer geometry: points, directions, map dimensions.
//!
//! Port of `max-cpp-port/src/point.hpp` and `path.hpp`:
//! * [`Point`] — `int16_t x, y` (arithmetic via `int` with narrowing, as in C++);
//! * [`PathStep`] — `int8_t x, y` (path step);
//! * [`Direction`] — 8 directions clockwise from north;
//! * [`DIRECTION_OFFSETS`] — the `(dx, dy)` offsets of each direction.
//!
//! No `f32`/`f64`: all distances are integer (DESIGN.md §6.4).

use core::fmt;
use core::ops::{Add, AddAssign, Sub, SubAssign};

use serde::{Deserialize, Serialize};

use crate::enums::InvalidEnumValue;

/// Map width in tiles (WRL file; in V70 it is always 112).
pub const MAP_WIDTH: i16 = 112;
/// Map height in tiles.
pub const MAP_HEIGHT: i16 = 112;
/// Number of map tiles: `112 * 112 == 12544`.
pub const MAP_TILES: usize = MAP_WIDTH as usize * MAP_HEIGHT as usize;
/// Default map size (skeleton `map::MapSize`).
pub const MAP_SIZE: crate::map::MapSize = crate::map::MapSize {
    width: MAP_WIDTH,
    height: MAP_HEIGHT,
};

// ---------------------------------------------------------------------------
// Point / PathStep
// ---------------------------------------------------------------------------

/// Point/coordinate on the map grid — `Point { int16_t x, y; }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Point {
    /// X coordinate (west→east).
    pub x: i16,
    /// Y coordinate (north→south).
    pub y: i16,
}

impl Point {
    /// Zero point.
    pub const ZERO: Self = Self { x: 0, y: 0 };

    /// Creates a point.
    #[inline]
    pub const fn new(x: i16, y: i16) -> Self {
        Self { x, y }
    }

    /// Index of a tile in the flat `SurfaceMap` array at width [`MAP_WIDTH`].
    #[inline]
    pub const fn index(self) -> usize {
        self.y as usize * MAP_WIDTH as usize + self.x as usize
    }

    /// Point from a tile index (inverse of [`Point::index`] for `MAP_WIDTH`).
    #[inline]
    pub const fn from_index(index: usize) -> Self {
        Self {
            x: (index % MAP_WIDTH as usize) as i16,
            y: (index / MAP_WIDTH as usize) as i16,
        }
    }

    /// Manhattan distance (integer, without i16 overflow).
    #[inline]
    pub const fn manhattan_distance(self, other: Self) -> i32 {
        let dx = (self.x as i32 - other.x as i32).abs();
        let dy = (self.y as i32 - other.y as i32).abs();
        dx + dy
    }

    /// Chebyshev distance (maximum over the axes).
    #[inline]
    pub const fn chebyshev_distance(self, other: Self) -> i32 {
        let dx = (self.x as i32 - other.x as i32).abs();
        let dy = (self.y as i32 - other.y as i32).abs();
        if dx > dy {
            dx
        } else {
            dy
        }
    }

    /// Square of the Euclidean distance (computed in `i32` in C++).
    #[inline]
    pub const fn distance_squared(self, other: Self) -> i32 {
        let dx = self.x as i32 - other.x as i32;
        let dy = self.y as i32 - other.y as i32;
        dx * dx + dy * dy
    }

    /// Integer Euclidean distance (truncated, like `sqrt` in `i32`).
    #[inline]
    pub fn euclidean_distance(self, other: Self) -> i32 {
        // Integer square root: integer arithmetic only.
        let mut value = self.distance_squared(other);
        if value <= 0 {
            return 0;
        }
        let mut root = 0i32;
        let mut bit = 1i32 << 30;
        while bit > value {
            bit >>= 2;
        }
        while bit != 0 {
            if value >= root + bit {
                value -= root + bit;
                root = (root >> 1) + bit;
            } else {
                root >>= 1;
            }
            bit >>= 2;
        }
        root
    }

    /// Neighboring point in the given direction (i16 wrapping arithmetic,
    /// like the narrowing of `int` in C++).
    #[inline]
    pub const fn step(self, direction: Direction) -> Self {
        let offset = direction.offset();
        // C++: `short += short` is computed in `int` and narrowed on store.
        Self {
            x: (self.x as i32 + offset.x as i32) as i16,
            y: (self.y as i32 + offset.y as i32) as i16,
        }
    }

    /// Direction from `self` to `other` if the points lie on one of the 8 lines.
    pub fn direction_to(self, other: Self) -> Option<Direction> {
        let dx = (other.x as i32 - self.x as i32).signum();
        let dy = (other.y as i32 - self.y as i32).signum();
        if dx == 0 && dy == 0 {
            return None;
        }
        let raw = match (dx, dy) {
            (0, -1) => Direction::North,
            (1, -1) => Direction::NorthEast,
            (1, 0) => Direction::East,
            (1, 1) => Direction::SouthEast,
            (0, 1) => Direction::South,
            (-1, 1) => Direction::SouthWest,
            (-1, 0) => Direction::West,
            (-1, -1) => Direction::NorthWest,
            _ => return None,
        };
        Some(raw)
    }
}

impl Add for Point {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self {
            x: (self.x as i32 + rhs.x as i32) as i16,
            y: (self.y as i32 + rhs.y as i32) as i16,
        }
    }
}

impl AddAssign for Point {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for Point {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: (self.x as i32 - rhs.x as i32) as i16,
            y: (self.y as i32 - rhs.y as i32) as i16,
        }
    }
}

impl SubAssign for Point {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

/// Path step — `PathStep { int8_t x, y; }` (`point.hpp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct PathStep {
    /// X offset.
    pub x: i8,
    /// Y offset.
    pub y: i8,
}

impl PathStep {
    /// Creates a path step.
    #[inline]
    pub const fn new(x: i8, y: i8) -> Self {
        Self { x, y }
    }

    /// Converts the step into a [`Point`] offset.
    #[inline]
    pub const fn as_point(self) -> Point {
        Point {
            x: self.x as i16,
            y: self.y as i16,
        }
    }
}

// ---------------------------------------------------------------------------
// Direction
// ---------------------------------------------------------------------------

/// Direction on the grid — C++ `Direction` / `UnitAngleType`.
///
/// 8 values clockwise from north: N, NE, E, SE, S, SW, W, NW.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Direction {
    /// North (`DIRECTION_NORTH`).
    North = 0,
    /// Northeast.
    NorthEast = 1,
    /// East.
    East = 2,
    /// Southeast.
    SouthEast = 3,
    /// South.
    South = 4,
    /// Southwest.
    SouthWest = 5,
    /// West.
    West = 6,
    /// Northwest.
    NorthWest = 7,
}

impl Direction {
    /// All directions in C++ order.
    pub const ALL: [Self; 8] = [
        Self::North,
        Self::NorthEast,
        Self::East,
        Self::SouthEast,
        Self::South,
        Self::SouthWest,
        Self::West,
        Self::NorthWest,
    ];

    /// `DIRECTION_COUNT` from `path.hpp`.
    pub const COUNT: usize = 8;

    /// `(dx, dy)` offset of this direction.
    #[inline]
    pub const fn offset(self) -> Point {
        DIRECTION_OFFSETS[self as usize]
    }

    /// Decodes a value with a `& 7` mask (as `UnitsManager_GetTargetAngle`).
    #[inline]
    pub const fn from_raw_masked(raw: u8) -> Self {
        Self::ALL[(raw & 7) as usize]
    }

    /// Opposite direction (180° turn).
    #[inline]
    pub const fn opposite(self) -> Self {
        Self::ALL[((self as usize) + 4) & 7]
    }

    /// Rotate clockwise by 45°.
    #[inline]
    pub const fn rotate_cw(self) -> Self {
        Self::ALL[((self as usize) + 1) & 7]
    }

    /// Rotate counter-clockwise by 45°.
    #[inline]
    pub const fn rotate_ccw(self) -> Self {
        Self::ALL[((self as usize) + 7) & 7]
    }
}

impl From<Direction> for u8 {
    #[inline]
    fn from(value: Direction) -> Self {
        value as u8
    }
}

impl From<Direction> for u16 {
    #[inline]
    fn from(value: Direction) -> Self {
        value as u16
    }
}

impl TryFrom<u8> for Direction {
    type Error = InvalidEnumValue;

    #[inline]
    fn try_from(raw: u8) -> Result<Self, Self::Error> {
        Self::ALL
            .get(raw as usize)
            .copied()
            .ok_or_else(|| InvalidEnumValue::new("Direction", raw as u32))
    }
}

impl TryFrom<u16> for Direction {
    type Error = InvalidEnumValue;

    #[inline]
    fn try_from(raw: u16) -> Result<Self, Self::Error> {
        if raw <= u8::MAX as u16 {
            Self::try_from(raw as u8)
        } else {
            Err(InvalidEnumValue::new("Direction", raw as u32))
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::North => "N",
            Self::NorthEast => "NE",
            Self::East => "E",
            Self::SouthEast => "SE",
            Self::South => "S",
            Self::SouthWest => "SW",
            Self::West => "W",
            Self::NorthWest => "NW",
        })
    }
}

/// Direction offsets — C++ `DIRECTION_OFFSETS` (`path.hpp`).
///
/// Order: N, NE, E, SE, S, SW, W, NW (clockwise from north).
pub const DIRECTION_OFFSETS: [Point; Direction::COUNT] = [
    Point { x: 0, y: -1 },
    Point { x: 1, y: -1 },
    Point { x: 1, y: 0 },
    Point { x: 1, y: 1 },
    Point { x: 0, y: 1 },
    Point { x: -1, y: 1 },
    Point { x: -1, y: 0 },
    Point { x: -1, y: -1 },
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
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
}
