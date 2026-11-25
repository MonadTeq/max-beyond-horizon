//! Ordered hash tables: `UnitHash` (id → unit) and `MapHash`
//! (cell → units), as in `max-cpp-port/src/hash.cpp` / `hash.hpp`.
//!
//! Determinism matters more than speed: buckets are `Vec`s (insertion order), iteration
//! is always by ascending bucket index, and within a bucket by insertion order
//! (DESIGN §6.2). Hash functions and sizes are 1:1 with C++:
//!
//! * `UnitHash`: `hash_size = 8192` (`HASH_UNIT_HASH_SIZE`), a bucket =
//!   `id % hash_size`;
//! * `MapHash`: size 512..32768 (`HASH_MAP_HASH_SIZE_MIN/MAX`), a bucket =
//!   `(grid_y ^ (grid_x << x_shift)) % hash_size`, where `x_shift` grows while
//!   `size > 128` (`MapHash::Init`); `SetMapSize` takes ~8 cells per bucket.
//!
//! Bucket values are an arbitrary key `T` (`UnitId` for the domain model,
//! `u16` — the object number in the V70 file, see `save.rs`).

use crate::classification::Footprint;
use crate::geo::Point;
use crate::unit::UnitId;

/// `UnitHash` size (`HASH_UNIT_HASH_SIZE`).
pub const HASH_UNIT_HASH_SIZE: u16 = 8192;
/// Minimum `MapHash` size (`HASH_MAP_HASH_SIZE_MIN`).
pub const HASH_MAP_HASH_SIZE_MIN: u16 = 512;
/// Maximum `MapHash` size (`HASH_MAP_HASH_SIZE_MAX`).
pub const HASH_MAP_HASH_SIZE_MAX: u16 = 32768;

/// `UnitHash` — `hash_size` ordered buckets keyed by `id % hash_size`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitHash<T = UnitId> {
    size: u16,
    buckets: Vec<Vec<T>>,
}

impl<T> UnitHash<T> {
    /// Creates a hash of the given size (`UnitHash::UnitHash`).
    pub fn new(size: u16) -> Self {
        Self {
            size,
            buckets: (0..size).map(|_| Vec::new()).collect(),
        }
    }

    /// Table size.
    #[inline]
    pub const fn size(&self) -> u16 {
        self.size
    }

    /// Buckets by index order (C++ iteration order).
    #[inline]
    pub fn buckets(&self) -> &[Vec<T>] {
        &self.buckets
    }

    /// Mutable access to the buckets (for the save parser).
    #[inline]
    pub fn buckets_mut(&mut self) -> &mut [Vec<T>] {
        &mut self.buckets
    }

    /// Bucket by index (`None` out of range).
    #[inline]
    pub fn bucket(&self, index: u16) -> Option<&Vec<T>> {
        self.buckets.get(index as usize)
    }

    /// Mutable bucket by index.
    #[inline]
    pub fn bucket_mut(&mut self, index: u16) -> Option<&mut Vec<T>> {
        self.buckets.get_mut(index as usize)
    }

    /// Pushes a value to the end of a bucket (`idx % size`).
    pub fn push_at(&mut self, key: u32, value: T) {
        let index = (key % u32::from(self.size)) as usize;
        self.buckets[index].push(value);
    }

    /// Removes the first occurrence of a value from its bucket.
    pub fn remove_at(&mut self, key: u32, value: &T) -> bool
    where
        T: PartialEq,
    {
        let index = (key % u32::from(self.size)) as usize;
        let bucket = &mut self.buckets[index];
        if let Some(position) = bucket.iter().position(|item| item == value) {
            bucket.remove(position);
            true
        } else {
            false
        }
    }

    /// Clears all buckets (`UnitHash::Clear`).
    pub fn clear(&mut self) {
        for bucket in &mut self.buckets {
            bucket.clear();
        }
    }

    /// Iterates all values: buckets in order, insertion order within each.
    pub fn iter_values(&self) -> impl Iterator<Item = &T> {
        self.buckets.iter().flat_map(|bucket| bucket.iter())
    }
}

impl UnitHash<UnitId> {
    /// `UnitHash` for the domain model: 8192 buckets, key — [`UnitId`].
    pub fn with_default_size() -> Self {
        Self::new(HASH_UNIT_HASH_SIZE)
    }

    /// `UnitHash::PushBack`: into bucket `id % hash_size`.
    pub fn push_back(&mut self, id: UnitId) {
        self.push_at(u32::from(id.0), id);
    }

    /// `UnitHash::Remove`.
    pub fn remove(&mut self, id: UnitId) -> bool {
        self.remove_at(u32::from(id.0), &id)
    }

    /// `UnitHash::operator[]`: lookup by id (exact match within the bucket).
    pub fn get(&self, id: UnitId) -> Option<UnitId> {
        let bucket = self.bucket((u32::from(id.0) % u32::from(self.size)) as u16)?;
        bucket.iter().copied().find(|item| *item == id)
    }
}

/// `MapHash` entry: a cell and the list of units on it (`MapHashObject`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapHashObject<T = UnitId> {
    /// Cell X.
    pub x: u16,
    /// Cell Y.
    pub y: u16,
    /// Units on the cell — insertion order (`SmartList<UnitInfo>`).
    pub units: Vec<T>,
}

impl<T> MapHashObject<T> {
    /// An empty cell object.
    pub fn new(x: u16, y: u16) -> Self {
        Self {
            x,
            y,
            units: Vec::new(),
        }
    }

    /// Whether the cell matches.
    #[inline]
    pub fn matches(&self, x: u16, y: u16) -> bool {
        self.x == x && self.y == y
    }
}

/// `MapHash` — buckets of `MapHashObject` lists from `hash.cpp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapHash<T = UnitId> {
    size: u16,
    x_shift: u16,
    buckets: Vec<Vec<MapHashObject<T>>>,
}

impl<T> MapHash<T> {
    /// Creates a table of size `size` (`MapHash::MapHash` + `Init`).
    pub fn new(size: u16) -> Self {
        let mut x_shift = 0u16;
        let mut reduced = size;
        while reduced > 128 {
            x_shift += 1;
            reduced >>= 1;
        }
        Self {
            size,
            x_shift,
            buckets: (0..size).map(|_| Vec::new()).collect(),
        }
    }

    /// `MapHash::SetMapSize`: ~8 map cells per bucket.
    pub fn set_map_size(map_size: Point) -> Self {
        let mut size = HASH_MAP_HASH_SIZE_MIN;
        let cells = u32::from(map_size.x.max(0) as u16) * u32::from(map_size.y.max(0) as u16);
        while size < HASH_MAP_HASH_SIZE_MAX && u32::from(size) * 8 < cells {
            size <<= 1;
        }
        Self::new(size)
    }

    /// Table size (`MapHash::hash_size`).
    #[inline]
    pub const fn size(&self) -> u16 {
        self.size
    }

    /// `x` shift for the hash function (`MapHash::x_shift`).
    #[inline]
    pub const fn x_shift(&self) -> u16 {
        self.x_shift
    }

    /// Buckets by index order.
    #[inline]
    pub fn buckets(&self) -> &[Vec<MapHashObject<T>>] {
        &self.buckets
    }

    /// Mutable access to the buckets (save parser).
    #[inline]
    pub fn buckets_mut(&mut self) -> &mut [Vec<MapHashObject<T>>] {
        &mut self.buckets
    }

    /// Bucket index for a cell: `(y ^ (x << x_shift)) % size`.
    #[inline]
    pub fn bucket_index(&self, x: u16, y: u16) -> u16 {
        ((y ^ (x << self.x_shift)) % self.size) as u16
    }

    /// Bucket of a cell.
    pub fn bucket(&self, x: u16, y: u16) -> &[MapHashObject<T>] {
        &self.buckets[self.bucket_index(x, y) as usize]
    }

    /// Mutable bucket of a cell.
    pub fn bucket_mut(&mut self, x: u16, y: u16) -> &mut Vec<MapHashObject<T>> {
        let index = self.bucket_index(x, y) as usize;
        &mut self.buckets[index]
    }

    /// Clears the table (`MapHash::Clear`).
    pub fn clear(&mut self) {
        for bucket in &mut self.buckets {
            bucket.clear();
        }
    }

    /// Iterates all cell objects: buckets in order, insertion order within each.
    pub fn iter_objects(&self) -> impl Iterator<Item = &MapHashObject<T>> {
        self.buckets.iter().flat_map(|bucket| bucket.iter())
    }
}

impl MapHash<UnitId> {
    /// `MapHash` for the 112×112 map (`SetMapSize` yields 512 buckets, `x_shift = 2`).
    pub fn for_standard_map() -> Self {
        Self::set_map_size(crate::geo::MAP_SIZE.as_point())
    }

    /// `MapHash::AddEx` (`hash.cpp:153`): inserts a unit into the cell object
    /// (creating it if needed). C++ pushes to the **front** when `!mode`
    /// (`if (!mode || unit->GetUnitType() == LRGTAPE || unit->GetUnitType() ==
    /// SMLTAPE) { PushFront } else { PushBack }`), so `mode == false` puts the
    /// newest unit first and `mode == true` appends it.
    ///
    /// The `LRGTAPE`/`SMLTAPE` exception of the oracle is not representable
    /// here: the table is keyed by [`UnitId`], not `UnitInfo`, so the unit type
    /// is unknown. Every port caller passes `mode = false` (the push-front
    /// branch), which is also the oracle branch for tapes.
    pub fn add_ex(&mut self, id: UnitId, grid_x: u16, grid_y: u16, mode: bool) {
        let bucket = self.bucket_mut(grid_x, grid_y);
        let position = match bucket.iter().position(|object| object.matches(grid_x, grid_y)) {
            Some(position) => position,
            None => {
                // C++ `PushFront` of a new cell object.
                bucket.insert(0, MapHashObject::new(grid_x, grid_y));
                0
            }
        };
        if mode {
            bucket[position].units.push(id);
        } else {
            bucket[position].units.insert(0, id);
        }
    }

    /// `MapHash::Add`: the unit's cell, and for 2×2 structures ([`Footprint::Quad`])
    /// three more cells.
    pub fn add(&mut self, id: UnitId, grid_x: u16, grid_y: u16, footprint: Footprint, mode: bool) {
        self.add_ex(id, grid_x, grid_y, mode);
        if footprint.is_quad() {
            self.add_ex(id, grid_x.wrapping_add(1), grid_y, mode);
            self.add_ex(id, grid_x, grid_y.wrapping_add(1), mode);
            self.add_ex(id, grid_x.wrapping_add(1), grid_y.wrapping_add(1), mode);
        }
    }

    /// `MapHash::RemoveEx`: removes the unit from the cell, deleting an empty object.
    pub fn remove_ex(&mut self, id: UnitId, grid_x: u16, grid_y: u16) {
        let bucket = self.bucket_mut(grid_x, grid_y);
        if let Some(position) = bucket.iter().position(|object| object.matches(grid_x, grid_y)) {
            bucket[position].units.retain(|item| *item != id);
            if bucket[position].units.is_empty() {
                bucket.remove(position);
            }
        }
    }

    /// `MapHash::Remove`: the unit's cell, and for 2×2 structures ([`Footprint::Quad`])
    /// three more cells.
    pub fn remove(&mut self, id: UnitId, grid_x: u16, grid_y: u16, footprint: Footprint) {
        self.remove_ex(id, grid_x, grid_y);
        if footprint.is_quad() {
            self.remove_ex(id, grid_x.wrapping_add(1), grid_y);
            self.remove_ex(id, grid_x, grid_y.wrapping_add(1));
            self.remove_ex(id, grid_x.wrapping_add(1), grid_y.wrapping_add(1));
        }
    }

    /// `MapHash::operator[]`: the list of units on a cell.
    pub fn get(&self, x: u16, y: u16) -> Option<&Vec<UnitId>> {
        self.bucket(x, y)
            .iter()
            .find(|object| object.matches(x, y))
            .map(|object| &object.units)
    }
}

/// `Point` → map-size `Point` (a convenience adapter for `MapHash::set_map_size`).
trait MapSizeExt {
    fn as_point(self) -> Point;
}

impl MapSizeExt for crate::map::MapSize {
    fn as_point(self) -> Point {
        Point::new(self.width, self.height)
    }
}

#[cfg(test)]
#[path = "hash_tests.rs"]
mod tests;
