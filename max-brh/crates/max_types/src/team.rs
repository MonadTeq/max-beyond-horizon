//! Листовые типы команд (Infrastructure).
//!
//! Здесь только типы, не читающие данных `max_core`: [`TeamId`], [`TeamKind`]
//! и [`Team`]. Состояние команды (`TeamInfo`, `TeamInfoUi`), экономика
//! (`TeamUnits`) и таблица атрибутов (`UnitValues`) остаются в
//! `max_core::team` — они зависят от данных симуляции.
//!
//! Оракул: `max-cpp-port/src/enums.hpp` (`PlayerTeamType`, `TeamClanType`),
//! `ctinfo.hpp` (`CTInfo`), `teamunits.hpp` (`TeamUnits`, `Complex`, `Cargo`).

use serde::{Deserialize, Serialize};

use crate::enums::{ResearchTopic, RESEARCH_TOPIC_COUNT};

/// Идентификатор команды (0-3 у оригинала, 4 = пришельцы).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TeamId(pub u8);

/// Кем управляется команда (C++ `PlayerType`, save.md `TeamType`).
///
/// Значения зафиксированы как в C++/V70: `None=0`, `Player=1` (HUMAN),
/// `Computer=2`, `Remote=3`, `Eliminated=4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum TeamKind {
    /// Пустой слот.
    None = 0,
    /// Локальный игрок (`TEAM_TYPE_HUMAN`).
    Player = 1,
    /// Компьютер (ИИ симулируется сервером).
    Computer = 2,
    /// Удалённый игрок (другой клиент).
    Remote = 3,
    /// Выбыла.
    Eliminated = 4,
}

/// Команда — минимальный каркас для потребителей (`max_net`, `max_render`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    /// Индекс команды.
    pub id: TeamId,
    /// Тип команды.
    pub kind: TeamKind,
    /// Имя команды.
    pub name: String,
    /// Клан команды (сырое значение `TeamClan`).
    pub clan: u8,
    /// Золото команды.
    pub gold: i32,
    /// Стартовые кредиты.
    pub start_gold: i32,
}

impl Team {
    /// Создаёт команду с настройками по умолчанию.
    pub fn new(id: TeamId, kind: TeamKind) -> Self {
        Self {
            id,
            kind,
            name: String::new(),
            clan: 0,
            gold: 0,
            start_gold: 0,
        }
    }
}

/// `TeamUnits::hash_team_id` — бит цвета команды в флагах юнита.
///
/// C++ `UnitInfo::UnitInfo` (`unitinfo.cpp:179`) собирает флаги как
/// `unit_flags | UnitsManager_TeamInfo[team].team_units->hash_team_id`, а
/// `hash_team_id` выставляется слотам в `ResourceManager_InitTeamUnits`
/// (`resource_manager.cpp:969-981`): RED = `HASH_TEAM_RED` 0x2000,
/// GREEN = 0x1000, BLUE = 0x800, GRAY = 0x400, ALIEN/derelict = 0x8000.
/// Именно эти биты читают рамка `COLORS` (`DrawMap_RenderUnitDisplays`,
/// `drawmap.cpp:581-598`), мини-карта (`drawmap.cpp:844-858`) и перекраска
/// спрайта (`unitinfo.cpp:3112-3126`), а `team_clan` влияет только на кадры
/// шахты (`unitinfo.cpp:282`) и торговые модификаторы клана.
///
/// Порт **не пишет** эти биты в `UnitInfo::flags` при спавне (набор флагов
/// входит в канонический дайджест, а эталонный C++-вектор собран без них) —
/// слой отрисовки получает тот же цвет из этого отображения
/// (`max_app::overlays::team_color_index`), а биты из загруженных сейвов
/// honoured напрямую.
pub const fn hash_team_id(team: TeamId) -> u16 {
    use crate::enums::unit_flags;

    match team.0 {
        0 => unit_flags::HASH_TEAM_RED as u16,
        1 => unit_flags::HASH_TEAM_GREEN as u16,
        2 => unit_flags::HASH_TEAM_BLUE as u16,
        3 => unit_flags::HASH_TEAM_GRAY as u16,
        _ => unit_flags::HASH_TEAM_DERELICT as u16,
    }
}

/// Позиция экрана — C++ `struct ScreenLocation` (`ctinfo.hpp`, UI).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenLocation {
    /// Координата X.
    pub x: i8,
    /// Координата Y.
    pub y: i8,
}

/// Прогресс исследования — C++ `struct ResearchTopic` (`ctinfo.hpp`),
/// save.md `ResearchTopicInfo`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchTopicInfo {
    /// Завершённых циклов исследования.
    pub research_level: u32,
    /// Ходов на цикл при одном исследовательском центре (0 = не начато).
    pub turns_to_complete: u32,
    /// Сколько исследовательских центров выделено.
    pub allocation: u32,
}

impl ResearchTopicInfo {
    /// Фактическое число ходов до завершения:
    /// `ceil(turns_to_complete / allocation)` (`researchmenu.cpp:448`:
    /// `(allocation + turns_to_complete - 1) / allocation`); 0 при `allocation == 0`.
    pub fn turns_remaining(&self) -> u32 {
        if self.allocation == 0 {
            0
        } else {
            (self.allocation + self.turns_to_complete - 1) / self.allocation
        }
    }

    /// Прогресс одного хода — C++ `game_manager.cpp:2072` /
    /// `ResearchMenu_UpdateResearchProgress(team, topic, 0)`: если тема
    /// выделена, но прогресс не начат — он пересчитывается симуляцией (P4).
    pub fn is_started(&self) -> bool {
        self.turns_to_complete > 0
    }
}

/// Идентификатор комплекса (`Complex::id`, `int16_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ComplexId(pub i16);

/// Комплекс зданий — C++ `class Complex : FileObject` (`complex.hpp`).
///
/// Комплекс — связная группа зданий, делящих энергию и грузы.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Complex {
    /// Материалы (запас + добыча).
    pub material: i16,
    /// Топливо.
    pub fuel: i16,
    /// Золото.
    pub gold: i16,
    /// Излишек энергии.
    pub power: i16,
    /// Свободная рабочая сила.
    pub workers: i16,
    /// Зданий в комплексе.
    pub buildings: i16,
    /// Уникальный индекс комплекса внутри команды.
    pub id: i16,
}

impl Complex {
    /// `Complex::Complex(id)`: все запасы 0, `buildings = 0`.
    pub const fn new(id: i16) -> Self {
        Self {
            material: 0,
            fuel: 0,
            gold: 0,
            power: 0,
            workers: 0,
            buildings: 0,
            id,
        }
    }

    /// Идентификатор комплекса.
    #[inline]
    pub const fn complex_id(&self) -> ComplexId {
        ComplexId(self.id)
    }

    /// `Complex::Grow` — здание подключилось к комплексу.
    pub fn grow(&mut self) {
        self.buildings = self.buildings.wrapping_add(1);
    }

    /// `Complex::Shrink` — здание отключилось; `true`, если комплекс опустел
    /// (C++ удаляет его из списка команды).
    pub fn shrink(&mut self) -> bool {
        self.buildings = self.buildings.wrapping_sub(1);
        self.buildings == 0
    }
}

/// Груз/запас — C++ `class Cargo` (`cargo.hpp`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cargo {
    /// Материалы.
    pub raw: i32,
    /// Топливо.
    pub fuel: i32,
    /// Золото.
    pub gold: i32,
    /// Свободная вместимость.
    pub free_capacity: i32,
}

impl Cargo {
    /// `Cargo::Init` — все поля 0.
    #[inline]
    pub const fn init() -> Self {
        Self {
            raw: 0,
            fuel: 0,
            gold: 0,
            free_capacity: 0,
        }
    }
}

/// Темы исследований в порядке `ATTRIB_*` (для итерации по 8 темам).
pub const RESEARCH_TOPICS: [ResearchTopic; RESEARCH_TOPIC_COUNT] = [
    ResearchTopic::Attack,
    ResearchTopic::Shots,
    ResearchTopic::Range,
    ResearchTopic::Armor,
    ResearchTopic::Hits,
    ResearchTopic::Speed,
    ResearchTopic::Scan,
    ResearchTopic::Cost,
];
