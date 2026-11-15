//! Юниты: идентификаторы, пути, `UnitInfo` (симуляционные поля).
//!
//! Оракул: `max-cpp-port/src/unitinfo.hpp` / `unitinfo.cpp` (`class UnitInfo`,
//! `FileLoad`/`FileSave`), `unitpath.hpp`, `groundpath.hpp`, `airpath.hpp`,
//! `builderpath.hpp` (данные путей), `point.hpp` (`PathStep`), `unit.hpp`
//! (флаги/`SfxType`).
//!
//! Проекция полей — `docs/state-model.md` §3.9/§6.4: `S`-поля входят в модель
//! симуляции; UI-поля (пиксельная позиция, спрайты, углы анимации, курсор)
//! сохранены только как сырые значения V70, потому что они лежат в том же
//! сейве и нужны для побайтового round-trip (`save.rs`). Симуляция их не
//! использует.

use serde::{Deserialize, Serialize};

use crate::enums::{ResearchTopic, UnitOrder, UnitOrderState, UnitType};
use crate::geo::{PathStep, Point};
use crate::team::{ComplexId, TeamId};
use crate::unit_values::UnitValues;

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

/// Прямоугольник в пикселях — C++ `struct Rect` (`point.hpp`/`gnw.h`).
///
/// В симуляции не участвует: кэш габаритов спрайта (UI), сохраняется в V70.
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

// ---------------------------------------------------------------------------
// Пути (UnitPath)
// ---------------------------------------------------------------------------

/// Наземный путь — C++ `class GroundPath : UnitPath` (`groundpath.hpp`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundPath {
    /// Конечная клетка X (`UnitPath::m_end_x`).
    pub end_x: i16,
    /// Конечная клетка Y (`UnitPath::m_end_y`).
    pub end_y: i16,
    /// Индекс текущего шага (в V70 — u16, в C++ — `uint32_t`).
    pub step_index: u32,
    /// Шаги пути в порядке следования.
    pub steps: Vec<PathStep>,
}

/// Воздушный путь — C++ `class AirPath : UnitPath` (`airpath.hpp`) — линейный
/// отрезок в пиксельных координатах.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AirPath {
    /// Длина (`m_length`).
    pub length: i16,
    /// Угол (`m_angle`, `char` = i8).
    pub angle: i8,
    /// Старт X (`m_start_x`).
    pub start_x: i16,
    /// Старт Y (`m_start_y`).
    pub start_y: i16,
    /// Конец X.
    pub end_x: i16,
    /// Конец Y.
    pub end_y: i16,
    /// Шаг X (`m_step_x`).
    pub step_x: i32,
    /// Шаг Y (`m_step_y`).
    pub step_y: i32,
    /// Дельта X (`m_delta_x`).
    pub delta_x: i32,
    /// Дельта Y (`m_delta_y`).
    pub delta_y: i32,
}

/// Путь строителя — C++ `class BuilderPath : UnitPath` (`builderpath.hpp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuilderPath {
    /// Направление X (`m_direction_x`).
    pub direction_x: i16,
    /// Направление Y (`m_direction_y`).
    pub direction_y: i16,
}

impl Default for BuilderPath {
    /// C++ `BuilderPath::BuilderPath` — направление (1, 1).
    fn default() -> Self {
        Self {
            direction_x: 1,
            direction_y: 1,
        }
    }
}

/// Путь юнита — C++ `UnitPath` (три реализации).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitPath {
    /// `AirPath`.
    Air(AirPath),
    /// `BuilderPath`.
    Builder(BuilderPath),
    /// `GroundPath`.
    Ground(GroundPath),
}

impl UnitPath {
    /// Конечная клетка (`UnitPath::GetEndX/GetEndY`).
    #[inline]
    pub fn end(&self) -> (i16, i16) {
        match self {
            Self::Air(path) => (path.end_x, path.end_y),
            Self::Builder(path) => (i16::from(path.direction_x), i16::from(path.direction_y)),
            Self::Ground(path) => (path.end_x, path.end_y),
        }
    }

    /// Для наземного пути — количество оставшихся шагов.
    #[inline]
    pub fn remaining_steps(&self) -> usize {
        match self {
            Self::Ground(path) => path.steps.len().saturating_sub(path.step_index as usize),
            _ => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// UnitInfo
// ---------------------------------------------------------------------------

/// Юнит — C++ `class UnitInfo` (`unitinfo.hpp`), симуляционное подмножество
/// `S`-полей (state-model §3.9).
///
/// Поля, помеченные «UI/V70», в симуляции не участвуют: они присутствуют
/// потому, что лежат в том же сериализованном объекте V70, и нужны для
/// побайтового round-trip сейва.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitInfo {
    // --- идентичность и положение ---
    /// Тип юнита.
    pub unit_type: UnitType,
    /// Уникальный id (`hash_id`; `NO_ID` = 0xFFFF).
    pub id: UnitId,
    /// Флаги (`unit_flags::*`): класс юнита, поведение, команда-владелец в хэше.
    pub flags: u32,
    /// Пиксельная позиция X (UI/V70: анимация).
    pub x: u16,
    /// Пиксельная позиция Y (UI/V70).
    pub y: u16,
    /// Клетка карты.
    pub grid: Point,
    /// Имя юнита (в V70 — переменной длины, без терминатора).
    pub name: String,
    /// Смещение тени (UI/V70).
    pub shadow_offset: Point,
    /// Команда-владелец (в т.ч. [`TeamId::ALIEN`] = 4).
    pub team: TeamId,
    /// Порядковый номер типа в команде (для имён).
    pub unit_id: u8,
    /// Яркость (UI/V70: визуал повреждения).
    pub brightness: u8,
    /// Угол поворота 0..7 (UI/V70; в симуляции — `Direction`).
    pub angle: u8,
    /// Видимость для каждой из 5 команд (туман войны).
    pub visible_to_team: [bool; 5],
    /// Обнаружен командами.
    pub spotted_by_team: [bool; 5],
    /// Максимальная скорость анимации (UI/V70).
    pub max_velocity: u8,
    /// Текущая скорость анимации (UI/V70).
    pub velocity: u8,
    /// Звуковой эффект (C++ `Unit::SfxType`, u8) — UI/V70.
    pub sound: u8,
    /// Масштаб спрайта (UI/V70).
    pub scaler_adjust: u8,
    /// Кэш габаритов спрайта (UI/V70).
    pub sprite_bounds: Rect,
    /// Кэш габаритов тени (UI/V70).
    pub shadow_bounds: Rect,
    /// Угол башни (UI/V70).
    pub turret_angle: u8,
    /// Смещение башни X (UI/V70).
    pub turret_offset_x: i8,
    /// Смещение башни Y (UI/V70).
    pub turret_offset_y: i8,
    /// Всего кадров (UI/V70).
    pub total_images: i16,
    /// База спрайта (UI/V70).
    pub image_base: i16,
    /// База спрайта башни (UI/V70).
    pub turret_image_base: i16,
    /// База спрайта выстрела (UI/V70).
    pub firing_image_base: i16,
    /// База спрайта коннектора (UI/V70).
    pub connector_image_base: i16,
    /// Текущий кадр (UI/V70).
    pub image_index: i16,
    /// Текущий кадр башни (UI/V70).
    pub turret_image_index: i16,
    /// Максимальный кадр (UI/V70).
    pub image_index_max: i16,

    // --- приказы ---
    /// Текущий приказ.
    pub orders: UnitOrder,
    /// Состояние исполнения приказа.
    pub state: UnitOrderState,
    /// Отложенный приказ (возврат после завершения текущего).
    pub prior_orders: UnitOrder,
    /// Отложенное состояние.
    pub prior_state: UnitOrderState,
    /// Маскировка/постановка мин (подлодка/миноукладчик).
    pub laying_state: u8,
    /// Целевая клетка (C++ `move_to_grid_*` = `fire_on_grid_*` в V70).
    pub target_grid: Point,

    // --- постройка и добыча ---
    /// Осталось ходов до конца постройки.
    pub build_time: u8,
    /// Суммарная добыча.
    pub total_mining: u8,
    /// Добыча материалов.
    pub raw_mining: u8,
    /// Добыча топлива.
    pub fuel_mining: u8,
    /// Добыча золота.
    pub gold_mining: u8,
    /// Максимум добычи материалов.
    pub raw_mining_max: u8,
    /// Максимум добычи золота.
    pub gold_mining_max: u8,
    /// Максимум добычи топлива.
    pub fuel_mining_max: u8,

    // --- боевые/экономические атрибуты ---
    /// Прочность (C++ u16, в V70 — u8).
    pub hits: u16,
    /// Скорость (C++ u16, в V70 — u8).
    pub speed: u16,
    /// Выстрелов в этом ходу.
    pub shots: u8,
    /// Стрельба на ходу.
    pub move_and_fire: u8,
    /// Груз; у `COMMANDO`/`REGENERATING_UNIT` — опыт (см. [`UnitInfo::experience`]).
    pub storage: i16,
    /// Опыт (в V70 хранится в `storage`).
    pub experience: i16,
    /// Переносимый груз (runtime; в V70 — `move_to_grid_x`).
    pub transfer_cargo: i16,
    /// Бросок стелса (runtime; в V70 — младший байт `move_to_grid_x`).
    pub stealth_dice_roll: u8,
    /// Боезапас.
    pub ammo: u8,
    /// Режим наведения (u8, не enum).
    pub targeting_mode: u8,
    /// Режим посадки/высадки (u8, не enum).
    pub enter_mode: u8,
    /// Курсор над юнитом (UI/V70).
    pub cursor: u8,
    /// Кадры отдачи выстрела (C++; в V70 — единый `recoil_delay`).
    pub firing_recoil_frames: u8,
    /// Оставшиеся ходы отключения (C++; в V70 — единый `recoil_delay`).
    pub disabled_turns_remaining: u8,
    /// Отложенная реакция (сетевая синхронизация).
    pub delayed_reaction: u8,
    /// Повреждён в этом ходу.
    pub damaged_this_turn: bool,
    /// Тема исследования здания.
    pub research_topic: ResearchTopic,
    /// Счётчик движения (лимит на ход).
    pub moved: u8,
    /// Покачивание на воде (UI/V70).
    pub bobbed: bool,
    /// Эффект тряски (UI/V70).
    pub shake_effect_state: u8,
    /// Состояние анимации двигателя (UI/V70).
    pub engine: u8,
    /// Состояние анимации оружия (UI/V70).
    pub weapon: u8,
    /// Не восстановленное C++-портом поле V70 (сохраняется как есть, §7.6).
    pub comm: u8,
    /// Не восстановленное C++-портом поле V70 (сохраняется как есть).
    pub fuel_distance: u8,
    /// Доля движения между клетками (UI/V70).
    pub move_fraction: u8,
    /// Не восстановленное C++-портом поле V70 (сохраняется как есть).
    pub energized: bool,
    /// Счётчик ожидания движения (лимит `MOVEMENT_WAIT_LIMIT` = 512).
    pub movement_wait_count: u16,
    /// Повторная постройка (завод) — C++ `uint8_t` с bool-семантикой.
    pub repeat_build: bool,
    /// Скорость постройки (множитель; 0 в файле трактуется как 1).
    pub build_rate: u16,
    /// Ответный огонь выключен.
    pub disabled_reaction_fire: bool,
    /// Авто-разведка.
    pub auto_survey: bool,
    /// AI-состояние (`AI_STATE_*`; в V70 — `field_221`).
    pub ai_state_bits: u32,

    // --- структура ---
    /// Маршрут движения.
    pub path: Option<UnitPath>,
    /// Битфилд подключений комплекса (`CONNECTOR_*`).
    pub connectors: u16,
    /// Атрибуты этого юнита (с апгрейдами) — C++ `base_values` (всегда есть).
    pub base_values: UnitValues,
    /// Принадлежность к комплексу (`None` — автономный).
    pub complex: Option<ComplexId>,
    /// Родитель (здание-завод/транспорт).
    pub parent_unit: Option<UnitId>,
    /// Текущая цель-враг.
    pub enemy_unit: Option<UnitId>,
    /// Очередь постройки завода (`UnitTypeArray`).
    pub build_list: Vec<UnitType>,

    // --- runtime-поля C++ (не сериализуются в V70) ---
    /// Скорость группы.
    pub group_speed: u16,
    /// Номер live-атаки (0 = нет).
    pub live_attack: u32,
    /// Юнит «в пути» (перемещается между клетками).
    pub in_transit: bool,
    /// Число «булавок» (пины задач).
    pub pin_count: i16,
    /// Задачи ИИ включены.
    pub tasks_enabled: bool,
    /// В каком из 5 списков юнит находится.
    pub unit_list: UnitListKind,
}

impl Default for UnitInfo {
    fn default() -> Self {
        Self::new(UnitType::CommTower, TeamId::RED, UnitId::NO_ID)
    }
}

impl UnitInfo {
    /// Минимальный конструктор (C++ `UnitInfo()` в «особых случаях»):
    /// заказы `AWAIT/INIT`, `brightness = 255`, позиция (-1, -1).
    pub fn new(unit_type: UnitType, team: TeamId, id: UnitId) -> Self {
        Self {
            unit_type,
            id,
            flags: 0,
            x: 0,
            y: 0,
            grid: Point::new(-1, -1),
            name: String::new(),
            shadow_offset: Point::ZERO,
            team,
            unit_id: 0,
            brightness: u8::MAX,
            angle: 0,
            visible_to_team: [false; 5],
            spotted_by_team: [false; 5],
            max_velocity: 0,
            velocity: 0,
            sound: 0,
            scaler_adjust: 0,
            sprite_bounds: Rect::default(),
            shadow_bounds: Rect::default(),
            turret_angle: 0,
            turret_offset_x: 0,
            turret_offset_y: 0,
            total_images: 0,
            image_base: 0,
            turret_image_base: 0,
            firing_image_base: 0,
            connector_image_base: 0,
            image_index: 0,
            turret_image_index: 0,
            image_index_max: 0,
            orders: UnitOrder::Await,
            state: UnitOrderState::Init,
            prior_orders: UnitOrder::Await,
            prior_state: UnitOrderState::Init,
            laying_state: 0,
            target_grid: Point::ZERO,
            build_time: 0,
            total_mining: 0,
            raw_mining: 0,
            fuel_mining: 0,
            gold_mining: 0,
            raw_mining_max: 0,
            gold_mining_max: 0,
            fuel_mining_max: 0,
            hits: 0,
            speed: 0,
            shots: 0,
            move_and_fire: 0,
            storage: 0,
            experience: 0,
            transfer_cargo: 0,
            stealth_dice_roll: 0,
            ammo: 0,
            targeting_mode: 0,
            enter_mode: 0,
            cursor: 0,
            firing_recoil_frames: 0,
            disabled_turns_remaining: 0,
            delayed_reaction: 0,
            damaged_this_turn: false,
            research_topic: ResearchTopic::Attack,
            moved: 0,
            bobbed: false,
            shake_effect_state: 0,
            engine: 0,
            weapon: 0,
            comm: 0,
            fuel_distance: 0,
            move_fraction: 0,
            energized: false,
            movement_wait_count: 0,
            repeat_build: false,
            build_rate: 1,
            disabled_reaction_fire: false,
            auto_survey: false,
            ai_state_bits: 0,
            path: None,
            connectors: 0,
            base_values: crate::unit_values::base_unit_values(unit_type),
            complex: None,
            parent_unit: None,
            enemy_unit: None,
            build_list: Vec::new(),
            group_speed: 0,
            live_attack: 0,
            in_transit: false,
            pin_count: 0,
            tasks_enabled: true,
            unit_list: UnitListKind::Stationary,
        }
    }

    /// Виден ли юнит команде (`UnitInfo::IsVisibleToTeam`).
    #[inline]
    pub fn is_visible_to_team(&self, team: TeamId) -> bool {
        self.visible_to_team
            .get(team.0 as usize)
            .copied()
            .unwrap_or(false)
    }

    /// Пометить видимость команде (`UnitInfo::SetVisibleToTeam`).
    #[inline]
    pub fn set_visible_to_team(&mut self, team: TeamId, visible: bool) {
        if let Some(slot) = self.visible_to_team.get_mut(team.0 as usize) {
            *slot = visible;
        }
    }

    /// Обнаружен ли юнит командой (`UnitInfo::IsDetectedByTeam`).
    #[inline]
    pub fn is_detected_by_team(&self, team: TeamId) -> bool {
        self.spotted_by_team
            .get(team.0 as usize)
            .copied()
            .unwrap_or(false)
    }

    /// Пометить обнаружение (`UnitInfo::SpotByTeam`).
    #[inline]
    pub fn spot_by_team(&mut self, team: TeamId) {
        if let Some(slot) = self.spotted_by_team.get_mut(team.0 as usize) {
            *slot = true;
        }
    }

    /// C++ `UnitInfo::GetId`.
    #[inline]
    pub const fn get_id(&self) -> UnitId {
        self.id
    }

    /// Опыт: у `COMMANDO`/`REGENERATING_UNIT` C++ хранит его в `storage`
    /// (state-model §3.9, DESIGN риск 16).
    #[inline]
    pub fn experience(&self) -> i16 {
        self.experience
    }

    /// Признак «юнит — член комплекса» (`UnitInfo::IsComplexMember`).
    #[inline]
    pub fn is_complex_member(&self) -> bool {
        use crate::enums::unit_flags::{BUILDING, CONNECTOR_UNIT, GROUND_COVER, STANDALONE};
        (self.flags & (CONNECTOR_UNIT | BUILDING | STANDALONE)) != 0
            && (self.flags & GROUND_COVER) == 0
            && !self.id.is_no_id()
    }

    /// Признак движущего приказа (`UnitInfo::HasMoveOrder`).
    #[inline]
    pub fn has_move_order(&self) -> bool {
        matches!(
            self.orders,
            UnitOrder::Move | UnitOrder::MoveToUnit | UnitOrder::MoveToAttack
        )
    }

    /// Список юнитов по флагам (`UnitInfo::GetUnitList`/`AllocateUnitList`).
    pub fn list_kind_for_flags(flags: u32) -> UnitListKind {
        use crate::enums::unit_flags::{
            GROUND_COVER, MOBILE_AIR_UNIT, MOBILE_LAND_UNIT, MOBILE_SEA_UNIT, STATIONARY,
        };
        if flags & GROUND_COVER != 0 {
            UnitListKind::GroundCover
        } else if flags & MOBILE_AIR_UNIT != 0 {
            UnitListKind::MobileAir
        } else if flags & (MOBILE_LAND_UNIT | MOBILE_SEA_UNIT) != 0 {
            UnitListKind::MobileLandSea
        } else if flags & STATIONARY != 0 {
            UnitListKind::Stationary
        } else {
            UnitListKind::Particle
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_id_bitfield_matches_cpp() {
        let id = UnitId::new(TeamId(2), 0x034);
        assert_eq!(id.team(), TeamId(2));
        assert_eq!(id.object_index(), 0x034);
        assert_eq!(id.0, (2 << 13) | 0x034);
        // Младшие 13 бит — индекс объекта: старшие биты отбрасываются.
        assert_eq!(UnitId::new(TeamId(2), 0x1234).object_index(), 0x1234);
        assert!(UnitId::NO_ID.is_no_id());
        assert_eq!(UnitId::from_v70(0xFFFF), UnitId::NO_ID);
        assert_eq!(UnitId::new(TeamId::ALIEN, 7).to_v70(), 0x8007);
    }

    #[test]
    fn unit_info_defaults_match_cpp() {
        let unit = UnitInfo::new(UnitType::Tank, TeamId::GREEN, UnitId::new(TeamId::GREEN, 1));
        assert_eq!(unit.orders, UnitOrder::Await);
        assert_eq!(unit.state, UnitOrderState::Init);
        assert_eq!(unit.brightness, u8::MAX);
        assert_eq!(unit.grid, Point::new(-1, -1));
        assert_eq!(unit.group_speed, 0);
        assert_eq!(unit.live_attack, 0);
        assert!(unit.tasks_enabled);
        assert_eq!(unit.build_rate, 1);
        assert_eq!(
            unit.base_values,
            crate::unit_values::base_unit_values(UnitType::Tank)
        );
        assert!(!unit.has_move_order());
        unit.visible_to_team
            .iter()
            .for_each(|visible| assert!(!visible));
    }

    #[test]
    fn visibility_and_detection_per_team() {
        let mut unit = UnitInfo::new(UnitType::Scout, TeamId::RED, UnitId::new(TeamId::RED, 2));
        assert!(!unit.is_visible_to_team(TeamId::RED));
        unit.set_visible_to_team(TeamId::RED, true);
        assert!(unit.is_visible_to_team(TeamId::RED));
        assert!(!unit.is_visible_to_team(TeamId::BLUE));
        assert!(!unit.is_detected_by_team(TeamId::RED));
        unit.spot_by_team(TeamId::RED);
        assert!(unit.is_detected_by_team(TeamId::RED));
    }

    #[test]
    fn complex_membership_matches_cpp_flags() {
        use crate::enums::unit_flags;
        let mut unit = UnitInfo::new(
            UnitType::PowerGenerator,
            TeamId::RED,
            UnitId::new(TeamId::RED, 5),
        );
        unit.flags = unit_flags::BUILDING;
        assert!(unit.is_complex_member());
        unit.flags |= unit_flags::GROUND_COVER;
        assert!(!unit.is_complex_member());
        unit.flags = unit_flags::STANDALONE;
        assert!(unit.is_complex_member());
        unit.id = UnitId::NO_ID;
        assert!(!unit.is_complex_member());
    }

    #[test]
    fn list_kinds_follow_cpp_order() {
        use crate::enums::unit_flags;
        assert_eq!(UnitListKind::ALL.len(), 5);
        assert_eq!(UnitListKind::GroundCover.index(), 0);
        assert_eq!(UnitListKind::from_index(4), Some(UnitListKind::Particle));
        assert_eq!(UnitListKind::from_index(5), None);
        assert_eq!(
            UnitInfo::list_kind_for_flags(unit_flags::GROUND_COVER),
            UnitListKind::GroundCover
        );
        assert_eq!(
            UnitInfo::list_kind_for_flags(unit_flags::MOBILE_AIR_UNIT),
            UnitListKind::MobileAir
        );
        assert_eq!(
            UnitInfo::list_kind_for_flags(unit_flags::MOBILE_SEA_UNIT),
            UnitListKind::MobileLandSea
        );
        assert_eq!(
            UnitInfo::list_kind_for_flags(unit_flags::STATIONARY),
            UnitListKind::Stationary
        );
        assert_eq!(UnitInfo::list_kind_for_flags(0), UnitListKind::Particle);
    }

    #[test]
    fn ground_path_tracks_remaining_steps() {
        let path = UnitPath::Ground(GroundPath {
            end_x: 10,
            end_y: 12,
            step_index: 1,
            steps: vec![
                PathStep::new(1, 1),
                PathStep::new(2, 2),
                PathStep::new(3, 3),
            ],
        });
        assert_eq!(path.end(), (10, 12));
        assert_eq!(path.remaining_steps(), 2);
        assert_eq!(UnitPath::Builder(BuilderPath::default()).end(), (1, 1));
    }
}
