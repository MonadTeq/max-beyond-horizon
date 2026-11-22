//! Упорядоченные хэш-таблицы: `UnitHash` (id → юнит) и `MapHash`
//! (клетка → юниты), как в `max-cpp-port/src/hash.cpp` / `hash.hpp`.
//!
//! Детерминизм важнее скорости: ведёрки — `Vec` (порядок вставки), итерация
//! всегда по возрастанию индекса ведёрка, внутри — порядок вставки
//! (DESIGN §6.2). Хэш-функции и размеры — 1:1 с C++:
//!
//! * `UnitHash`: `hash_size = 8192` (`HASH_UNIT_HASH_SIZE`), ведёрко =
//!   `id % hash_size`;
//! * `MapHash`: размер 512..32768 (`HASH_MAP_HASH_SIZE_MIN/MAX`), ведёрко =
//!   `(grid_y ^ (grid_x << x_shift)) % hash_size`, где `x_shift` растёт, пока
//!   `size > 128` (`MapHash::Init`); `SetMapSize` берёт ~8 клеток на ведёрко.
//!
//! Значения в ведёрках — произвольный ключ `T` (`UnitId` для доменной модели,
//! `u16` — номер объекта в файле V70, см. `save.rs`).

use crate::classification::Footprint;
use crate::geo::Point;
use crate::unit::UnitId;

/// Размер `UnitHash` (`HASH_UNIT_HASH_SIZE`).
pub const HASH_UNIT_HASH_SIZE: u16 = 8192;
/// Минимальный размер `MapHash` (`HASH_MAP_HASH_SIZE_MIN`).
pub const HASH_MAP_HASH_SIZE_MIN: u16 = 512;
/// Максимальный размер `MapHash` (`HASH_MAP_HASH_SIZE_MAX`).
pub const HASH_MAP_HASH_SIZE_MAX: u16 = 32768;

/// `UnitHash` — `hash_size` упорядоченных ведёрок по `id % hash_size`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitHash<T = UnitId> {
    size: u16,
    buckets: Vec<Vec<T>>,
}

impl<T> UnitHash<T> {
    /// Создаёт хэш указанного размера (`UnitHash::UnitHash`).
    pub fn new(size: u16) -> Self {
        Self {
            size,
            buckets: (0..size).map(|_| Vec::new()).collect(),
        }
    }

    /// Размер таблицы.
    #[inline]
    pub const fn size(&self) -> u16 {
        self.size
    }

    /// Ведёрки по порядку индекса (порядок итерации C++).
    #[inline]
    pub fn buckets(&self) -> &[Vec<T>] {
        &self.buckets
    }

    /// Изменяемый доступ к ведёркам (для парсера сейвов).
    #[inline]
    pub fn buckets_mut(&mut self) -> &mut [Vec<T>] {
        &mut self.buckets
    }

    /// Ведёрко по индексу (`None` вне диапазона).
    #[inline]
    pub fn bucket(&self, index: u16) -> Option<&Vec<T>> {
        self.buckets.get(index as usize)
    }

    /// Изменяемое ведёрко по индексу.
    #[inline]
    pub fn bucket_mut(&mut self, index: u16) -> Option<&mut Vec<T>> {
        self.buckets.get_mut(index as usize)
    }

    /// Кладёт значение в конец ведёрка (`idx % size`).
    pub fn push_at(&mut self, key: u32, value: T) {
        let index = (key % u32::from(self.size)) as usize;
        self.buckets[index].push(value);
    }

    /// Убирает первое вхождение значения из его ведёрка.
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

    /// Очищает все ведёрки (`UnitHash::Clear`).
    pub fn clear(&mut self) {
        for bucket in &mut self.buckets {
            bucket.clear();
        }
    }

    /// Обходит все значения: ведёрки по порядку, внутри — порядок вставки.
    pub fn iter_values(&self) -> impl Iterator<Item = &T> {
        self.buckets.iter().flat_map(|bucket| bucket.iter())
    }
}

impl UnitHash<UnitId> {
    /// `UnitHash` для доменной модели: 8192 ведёрка, ключ — [`UnitId`].
    pub fn with_default_size() -> Self {
        Self::new(HASH_UNIT_HASH_SIZE)
    }

    /// `UnitHash::PushBack`: в ведёрко `id % hash_size`.
    pub fn push_back(&mut self, id: UnitId) {
        self.push_at(u32::from(id.0), id);
    }

    /// `UnitHash::Remove`.
    pub fn remove(&mut self, id: UnitId) -> bool {
        self.remove_at(u32::from(id.0), &id)
    }

    /// `UnitHash::operator[]`: поиск по id (точное совпадение внутри ведёрка).
    pub fn get(&self, id: UnitId) -> Option<UnitId> {
        let bucket = self.bucket((u32::from(id.0) % u32::from(self.size)) as u16)?;
        bucket.iter().copied().find(|item| *item == id)
    }
}

/// Элемент `MapHash`: клетка и список юнитов на ней (`MapHashObject`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapHashObject<T = UnitId> {
    /// Клетка X.
    pub x: u16,
    /// Клетка Y.
    pub y: u16,
    /// Юниты на клетке — порядок вставки (`SmartList<UnitInfo>`).
    pub units: Vec<T>,
}

impl<T> MapHashObject<T> {
    /// Пустой объект клетки.
    pub fn new(x: u16, y: u16) -> Self {
        Self {
            x,
            y,
            units: Vec::new(),
        }
    }

    /// Совпадает ли клетка.
    #[inline]
    pub fn matches(&self, x: u16, y: u16) -> bool {
        self.x == x && self.y == y
    }
}

/// `MapHash` — ведёрки списков `MapHashObject` из `hash.cpp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapHash<T = UnitId> {
    size: u16,
    x_shift: u16,
    buckets: Vec<Vec<MapHashObject<T>>>,
}

impl<T> MapHash<T> {
    /// Создаёт таблицу размера `size` (`MapHash::MapHash` + `Init`).
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

    /// `MapHash::SetMapSize`: ~8 клеток карты на ведёрко.
    pub fn set_map_size(map_size: Point) -> Self {
        let mut size = HASH_MAP_HASH_SIZE_MIN;
        let cells = u32::from(map_size.x.max(0) as u16) * u32::from(map_size.y.max(0) as u16);
        while size < HASH_MAP_HASH_SIZE_MAX && u32::from(size) * 8 < cells {
            size <<= 1;
        }
        Self::new(size)
    }

    /// Размер таблицы (`MapHash::hash_size`).
    #[inline]
    pub const fn size(&self) -> u16 {
        self.size
    }

    /// Сдвиг `x` для хэш-функции (`MapHash::x_shift`).
    #[inline]
    pub const fn x_shift(&self) -> u16 {
        self.x_shift
    }

    /// Ведёрки по порядку индекса.
    #[inline]
    pub fn buckets(&self) -> &[Vec<MapHashObject<T>>] {
        &self.buckets
    }

    /// Изменяемый доступ к ведёркам (парсер сейвов).
    #[inline]
    pub fn buckets_mut(&mut self) -> &mut [Vec<MapHashObject<T>>] {
        &mut self.buckets
    }

    /// Индекс ведёрка для клетки: `(y ^ (x << x_shift)) % size`.
    #[inline]
    pub fn bucket_index(&self, x: u16, y: u16) -> u16 {
        ((y ^ (x << self.x_shift)) % self.size) as u16
    }

    /// Ведёрко клетки.
    pub fn bucket(&self, x: u16, y: u16) -> &[MapHashObject<T>] {
        &self.buckets[self.bucket_index(x, y) as usize]
    }

    /// Изменяемое ведёрко клетки.
    pub fn bucket_mut(&mut self, x: u16, y: u16) -> &mut Vec<MapHashObject<T>> {
        let index = self.bucket_index(x, y) as usize;
        &mut self.buckets[index]
    }

    /// Очищает таблицу (`MapHash::Clear`).
    pub fn clear(&mut self) {
        for bucket in &mut self.buckets {
            bucket.clear();
        }
    }

    /// Обходит все объекты клеток: ведёрки по порядку, внутри — порядок вставки.
    pub fn iter_objects(&self) -> impl Iterator<Item = &MapHashObject<T>> {
        self.buckets.iter().flat_map(|bucket| bucket.iter())
    }
}

impl MapHash<UnitId> {
    /// `MapHash` для карты 112×112 (`SetMapSize` даёт 512 ведёрок, `x_shift = 2`).
    pub fn for_standard_map() -> Self {
        Self::set_map_size(crate::geo::MAP_SIZE.as_point())
    }

    /// `MapHash::AddEx`: вставляет юнита в объект клетки (создавая его при
    /// необходимости), `push_front` при `mode == true`, иначе `push_back`.
    pub fn add_ex(&mut self, id: UnitId, grid_x: u16, grid_y: u16, mode: bool) {
        let bucket = self.bucket_mut(grid_x, grid_y);
        let position = match bucket.iter().position(|object| object.matches(grid_x, grid_y)) {
            Some(position) => position,
            None => {
                // C++ `PushFront` нового объекта клетки.
                bucket.insert(0, MapHashObject::new(grid_x, grid_y));
                0
            }
        };
        if mode {
            bucket[position].units.insert(0, id);
        } else {
            bucket[position].units.push(id);
        }
    }

    /// `MapHash::Add`: клетка юнита, у 2×2-структур ([`Footprint::Quad`]) — ещё
    /// три клетки.
    pub fn add(&mut self, id: UnitId, grid_x: u16, grid_y: u16, footprint: Footprint, mode: bool) {
        self.add_ex(id, grid_x, grid_y, mode);
        if footprint.is_quad() {
            self.add_ex(id, grid_x.wrapping_add(1), grid_y, mode);
            self.add_ex(id, grid_x, grid_y.wrapping_add(1), mode);
            self.add_ex(id, grid_x.wrapping_add(1), grid_y.wrapping_add(1), mode);
        }
    }

    /// `MapHash::RemoveEx`: убирает юнита из клетки, пустой объект удаляет.
    pub fn remove_ex(&mut self, id: UnitId, grid_x: u16, grid_y: u16) {
        let bucket = self.bucket_mut(grid_x, grid_y);
        if let Some(position) = bucket.iter().position(|object| object.matches(grid_x, grid_y)) {
            bucket[position].units.retain(|item| *item != id);
            if bucket[position].units.is_empty() {
                bucket.remove(position);
            }
        }
    }

    /// `MapHash::Remove`: клетка юнита, у 2×2-структур ([`Footprint::Quad`]) —
    /// ещё три клетки.
    pub fn remove(&mut self, id: UnitId, grid_x: u16, grid_y: u16, footprint: Footprint) {
        self.remove_ex(id, grid_x, grid_y);
        if footprint.is_quad() {
            self.remove_ex(id, grid_x.wrapping_add(1), grid_y);
            self.remove_ex(id, grid_x, grid_y.wrapping_add(1));
            self.remove_ex(id, grid_x.wrapping_add(1), grid_y.wrapping_add(1));
        }
    }

    /// `MapHash::operator[]`: список юнитов клетки.
    pub fn get(&self, x: u16, y: u16) -> Option<&Vec<UnitId>> {
        self.bucket(x, y)
            .iter()
            .find(|object| object.matches(x, y))
            .map(|object| &object.units)
    }
}

/// `Point` → `Point`-размер карты (удобный адаптер для `MapHash::set_map_size`).
trait MapSizeExt {
    fn as_point(self) -> Point;
}

impl MapSizeExt for crate::map::MapSize {
    fn as_point(self) -> Point {
        Point::new(self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_hash_buckets_and_order() {
        let mut hash = UnitHash::with_default_size();
        assert_eq!(hash.size(), HASH_UNIT_HASH_SIZE);
        let first = UnitId::new(crate::team::TeamId::RED, 5);
        let second = UnitId::new(crate::team::TeamId::GREEN, 5);
        hash.push_back(first);
        hash.push_back(second);
        // Ключ ведёрка — `id % hash_size`; обе команды дают 5 (`GREEN << 13` кратно 8192),
        // поэтому оба юнита лежат в одном ведёрке в порядке вставки.
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
        // C++ `SetMapSize` для 112×112: 512*8 < 12544 -> 1024 -> 2048 (2048*8 > 12544).
        assert_eq!(hash.size(), 4 * HASH_MAP_HASH_SIZE_MIN);
        // 2048 > 128 -> сдвиги: 1024, 512, 256, 128 => x_shift = 4.
        assert_eq!(hash.x_shift(), 4);
        assert_eq!(hash.bucket_index(0, 0), 0);
        assert_eq!(
            hash.bucket_index(3, 7),
            ((7u16 ^ (3 << 4)) % hash.size())
        );
        let big = MapHash::<UnitId>::set_map_size(Point::new(1000, 1000));
        assert_eq!(big.size(), HASH_MAP_HASH_SIZE_MAX);
        // Размер из V70-файла (512) даёт сдвиг 2 — как в реальном сейве.
        let saved = MapHash::<UnitId>::new(HASH_MAP_HASH_SIZE_MIN);
        assert_eq!(saved.x_shift(), 2);
    }

    #[test]
    fn map_hash_adds_buildings_to_four_cells() {
        let mut hash = MapHash::<UnitId>::for_standard_map();
        let id = UnitId::new(crate::team::TeamId::RED, 9);
        // Мобильный юнит — одна клетка.
        hash.add(id, 10, 10, Footprint::Single, false);
        assert_eq!(hash.get(10, 10), Some(&vec![id]));
        assert_eq!(hash.get(11, 10), None);
        hash.remove(id, 10, 10, Footprint::Single);
        assert_eq!(hash.get(10, 10), None);

        // Здание занимает 2×2.
        hash.add(id, 20, 20, Footprint::Quad, false);
        for (x, y) in [(20u16, 20u16), (21, 20), (20, 21), (21, 21)] {
            assert_eq!(hash.get(x, y), Some(&vec![id]), "cell {x},{y}");
        }
        hash.remove(id, 20, 20, Footprint::Quad);
        for (x, y) in [(20u16, 20u16), (21, 20), (20, 21), (21, 21)] {
            assert_eq!(hash.get(x, y), None, "cell {x},{y}");
        }
        // Пустой объект клетки удаляется (C++ `list->Remove(object)`).
        assert_eq!(hash.iter_objects().count(), 0);
    }

    #[test]
    fn map_hash_places_turret_in_single_cell() {
        use crate::classification::UnitTypeClassify;
        use crate::enums::UnitType;

        // Турель — `Mobility::Building`, но `Footprint::Single`: в `MapHash` она
        // занимает ровно одну клетку (не 2×2), как в C++ (флаг `BUILDING` снят).
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
    fn map_hash_keeps_insertion_order() {
        let mut hash = MapHash::<UnitId>::for_standard_map();
        let a = UnitId::new(crate::team::TeamId::RED, 1);
        let b = UnitId::new(crate::team::TeamId::RED, 2);
        hash.add(a, 5, 5, Footprint::Single, false);
        hash.add(b, 5, 5, Footprint::Single, false);
        assert_eq!(hash.get(5, 5), Some(&vec![a, b]));
        // `mode == true` — push_front (C++ для LRGTAPE/SMLTAPE).
        let mut hash = MapHash::<UnitId>::for_standard_map();
        hash.add(a, 5, 5, Footprint::Single, true);
        hash.add(b, 5, 5, Footprint::Single, true);
        assert_eq!(hash.get(5, 5), Some(&vec![b, a]));
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
}
