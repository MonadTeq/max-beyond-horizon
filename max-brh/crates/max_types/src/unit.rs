//! Идентификаторы юнитов и презентационные данные (Infrastructure).
//!
//! Здесь [`UnitId`] (16-битный id оригинала, старшие биты — команда),
//! [`UnitListKind`] — листовой тип без данных `max_core`, [`Rect`] —
//! прямоугольник в пикселях и [`UnitPresentation`] — презентационные поля
//! юнита (UI/рендер), которые не участвуют в симуляции, но лежат в том же
//! объекте V70 и потому нужны `max_core::save` для побайтового round-trip.
//!
//! Сам юнит (`UnitInfo`), его пути (`GroundPath`/`AirPath`/`BuilderPath`)
//! остаются в `max_core::unit`: они зависят от атрибутов и кадровых данных.
//!
//! Оракул: `max-cpp-port/src/unitinfo.hpp` (`UnitInfo::hash_id`),
//! `units_manager.cpp` (пять списков юнитов).

use serde::{Deserialize, Serialize};

use crate::geo::Point;
use crate::team::TeamId;

/// Идентификатор юнита (16-битный id оригинала).
///
/// Старшие биты — команда, младшие 13 бит — индекс объекта в команде
/// (`(team << 13) | object_index`), см. DESIGN.md §3.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UnitId(pub u16);

impl UnitId {
    /// `NO_ID` (0xFFFF) — отсутствие юнита.
    pub const NO_ID: Self = Self(0xFFFF);

    /// Собирает id из команды и индекса объекта.
    #[inline]
    pub const fn new(team: TeamId, object_index: u16) -> Self {
        Self(((team.0 as u16) << 13) | (object_index & 0x1FFF))
    }

    /// Команда из старших бит id.
    #[inline]
    pub const fn team(self) -> TeamId {
        TeamId((self.0 >> 13) as u8)
    }

    /// Индекс объекта в команде (младшие 13 бит).
    #[inline]
    pub const fn object_index(self) -> u16 {
        self.0 & 0x1FFF
    }

    /// Является ли id «пустым» (`NO_ID`).
    #[inline]
    pub const fn is_no_id(self) -> bool {
        self.0 == Self::NO_ID.0
    }

    /// Из `u16` с V70-семантикой: 0xFFFF = [`UnitId::NO_ID`].
    #[inline]
    pub const fn from_v70(raw: u16) -> Self {
        Self(raw)
    }

    /// В `u16` для записи в V70-файл.
    #[inline]
    pub const fn to_v70(self) -> u16 {
        self.0
    }
}

/// Один из пяти списков юнитов (`UnitInfo::unit_list`, `units_manager.cpp`).
///
/// Порядок вариантов — порядок обхода `UnitsManager_ProcessOrders`
/// (DESIGN §5.2): ground cover → mobile land/sea → stationary → mobile air →
/// particles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum UnitListKind {
    /// Наземное укрытие (`UnitsManager_GroundCoverUnits`).
    GroundCover = 0,
    /// Мобильные наземные и морские (`UnitsManager_MobileLandSeaUnits`).
    MobileLandSea = 1,
    /// Стационарные (`UnitsManager_StationaryUnits`).
    Stationary = 2,
    /// Мобильные воздушные (`UnitsManager_MobileAirUnits`).
    MobileAir = 3,
    /// Частицы/ракеты/снаряды (`UnitsManager_ParticleUnits`).
    Particle = 4,
}

impl UnitListKind {
    /// Все пять списков в каноническом порядке обхода.
    pub const ALL: [Self; 5] = [
        Self::GroundCover,
        Self::MobileLandSea,
        Self::Stationary,
        Self::MobileAir,
        Self::Particle,
    ];

    /// Индекс списка (0..5).
    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Список по индексу.
    #[inline]
    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::GroundCover),
            1 => Some(Self::MobileLandSea),
            2 => Some(Self::Stationary),
            3 => Some(Self::MobileAir),
            4 => Some(Self::Particle),
            _ => None,
        }
    }
}

/// Прямоугольник в пикселях — C++ `struct Rect` (`point.hpp`/`gnw.h`).
///
/// В симуляции не участвует: кэш габаритов спрайта (UI), сохраняется в V70.
/// Живёт в нижнем слое (перенесён из `max_core::unit`, Ф5.3), поэтому
/// [`UnitPresentation`] может ссылаться на него без зависимости вверх.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    /// Левый верхний X.
    pub ulx: i32,
    /// Левый верхний Y.
    pub uly: i32,
    /// Правый нижний X.
    pub lrx: i32,
    /// Правый нижний Y.
    pub lry: i32,
}

/// Презентационные поля юнита — C++ `class UnitInfo`, UI/V70-подмножество
/// (`state-model.md` §6.4; Ф5.3, часть B).
///
/// Поля не участвуют в симуляции, снапшоте, `replay_digest` и CRC пакета 45:
/// они читаются только `max_core::save` (побайтовый V70 round-trip) и
/// клиентским рендером (`max_app`). Проверка каждого поля — отчёт Ф5.3/B0.
///
/// Держится параллельно слотам `UnitInfo` в `max_core::game::GameState`
/// (презентация не входит в `UnitInfo`, чтобы прикладной слой не нёс
/// UI-данные). Логики нет — только конструктор.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitPresentation {
    /// Смещение тени (UI/V70).
    pub shadow_offset: Point,
    /// Яркость (UI/V70: визуал повреждения).
    pub brightness: u8,
    /// Звуковой эффект (C++ `Unit::SfxType`, u8) — UI/V70.
    pub sound: u8,
    /// Кэш габаритов спрайта (UI/V70).
    pub sprite_bounds: Rect,
    /// Кэш габаритов тени (UI/V70).
    pub shadow_bounds: Rect,
    /// Смещение башни X (UI/V70).
    pub turret_offset_x: i8,
    /// Смещение башни Y (UI/V70).
    pub turret_offset_y: i8,
    /// Всего кадров (UI/V70).
    pub total_images: i16,
    /// База спрайта выстрела (UI/V70).
    pub firing_image_base: i16,
    /// База спрайта коннектора (UI/V70).
    pub connector_image_base: i16,
    /// Курсор над юнитом (UI/V70).
    pub cursor: u8,
    /// Покачивание на воде (UI/V70).
    pub bobbed: bool,
}

impl UnitPresentation {
    /// Начальное состояние презентации — C++ `UnitInfo::UnitInfo`
    /// (`unitinfo.cpp:276-294`): `brightness = 255`, остальные поля нулевые.
    pub fn new() -> Self {
        Self {
            shadow_offset: Point::ZERO,
            brightness: u8::MAX,
            sound: 0,
            sprite_bounds: Rect::default(),
            shadow_bounds: Rect::default(),
            turret_offset_x: 0,
            turret_offset_y: 0,
            total_images: 0,
            firing_image_base: 0,
            connector_image_base: 0,
            cursor: 0,
            bobbed: false,
        }
    }
}

impl Default for UnitPresentation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C++ `UnitInfo::UnitInfo` (`unitinfo.cpp:276-294`): `brightness = 255`,
    /// остальные презентационные поля нулевые.
    #[test]
    fn presentation_defaults_match_cpp_unit_constructor() {
        let presentation = UnitPresentation::default();
        assert_eq!(presentation.brightness, u8::MAX);
        assert_eq!(presentation.shadow_offset, Point::ZERO);
        assert_eq!(presentation.sound, 0);
        assert_eq!(presentation.sprite_bounds, Rect::default());
        assert_eq!(presentation.shadow_bounds, Rect::default());
        assert_eq!(presentation.total_images, 0);
        assert_eq!(presentation.cursor, 0);
        assert!(!presentation.bobbed);
        assert_eq!(presentation, UnitPresentation::new());
    }
}
