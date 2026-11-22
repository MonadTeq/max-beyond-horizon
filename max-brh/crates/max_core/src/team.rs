//! Команды: типы, состояние команды (`TeamInfo`), экономика и технологии
//! (`TeamUnits`), комплексы (`Complex`).
//!
//! Оракул: `max-cpp-port/src/ctinfo.hpp` (`CTInfo`, `ResearchTopic`),
//! `teamunits.hpp/.cpp` (`TeamUnits`), `complex.hpp/.cpp` (`Complex`),
//! `cargo.hpp/.cpp` (`Cargo`), `enums.hpp` (`PlayerTeamType`, `TeamClanType`).
//!
//! Проекция `S`/`UI` — по `max-brh-docs/state-model.md` §3.4/§3.6/§3.7 и §6.3: UI-поля
//! `TeamInfo` (камера, зум, переключатели отображения, маркеры) в симуляцию не
//! входят; они собраны в [`TeamInfoUi`] и нужны только слою сейвов (V70).

use serde::{Deserialize, Serialize};

use crate::enums::{ResearchTopic, TeamClan, RESEARCH_TOPIC_COUNT, UNIT_END};
use crate::geo::Point;
use crate::unit_values::{UnitValues, UNIT_COUNT};

// Листовые типы команд (`TeamId`, `TeamKind`, `Team`) и их конверсии живут в
// нижнем слое (`max_types`, Ф5.2); здесь — реэкспорт, чтобы путь
// `max_core::team::…` не менялся.
pub use max_types::team::{Team, TeamId, TeamKind};

// Коннекторы комплекса и типы груза живут в `enums` (P0); здесь — реэкспорт,
// чтобы доменные типы команд/комплексов были самодостаточны.
pub use crate::enums::{
    CARGO_FUEL, CARGO_GOLD, CARGO_MASK, CARGO_MATERIALS, CONNECTION_BEING_TESTED,
    CONNECTOR_EAST_BOTTOM, CONNECTOR_EAST_TOP, CONNECTOR_NORTH_LEFT, CONNECTOR_NORTH_RIGHT,
    CONNECTOR_SOUTH_LEFT, CONNECTOR_SOUTH_RIGHT, CONNECTOR_WEST_BOTTOM, CONNECTOR_WEST_TOP,
};

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

// ---------------------------------------------------------------------------
// Вспомогательные структуры TeamInfo
// ---------------------------------------------------------------------------

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

/// Сколько ходов графика очков хранится (`score_graph[50]`).
pub const SCORE_GRAPH_LEN: usize = 50;

/// Количество позиций камеры в V70 (`camera_positions[6]`).
pub const CAMERA_POSITION_COUNT: usize = 6;

/// Количество маркеров в V70 (`markers[10]`, фича вырезана).
pub const MARKER_COUNT: usize = 10;

/// Количество display-переключателей `TeamInfo` (UI).
pub const DISPLAY_BUTTON_COUNT: usize = 11;

/// Состояние команды — C++ `struct CTInfo` (`ctinfo.hpp`), save.md `TeamInfo`.
///
/// Только `S`-поля (state-model §6.3); UI-поля — в [`TeamInfoUi`].
///
/// `serde`-производные не выводятся: массивы на 93/50 элементов не
/// поддерживаются стандартными impl-ами `serde` для массивов. Канонический
/// байтовый формат состояния — `save.rs`/`crc.rs` (DESIGN §2.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamInfo {
    /// Тип команды (`team_type`).
    pub team_type: TeamKind,
    /// Команда закончила ход.
    pub finished_turn: bool,
    /// Клан команды.
    pub team_clan: TeamClan,
    /// Прогресс 8 тем исследований.
    pub research_topics: [ResearchTopicInfo; RESEARCH_TOPIC_COUNT],
    /// Победные очки.
    pub team_points: u32,
    /// Счётчик созданных объектов 1..8191 — база для hash id
    /// (`number_of_objects_created`).
    pub number_of_objects_created: u16,
    /// Счётчики имён по типам (C++ `uint32_t[UNIT_END]`, в V70 — u8[93]).
    pub unit_counters: [u32; UNIT_COUNT],
    /// Очки команды за последние 50 ходов.
    pub score_graph: [i16; SCORE_GRAPH_LEN],
    /// Сколько заводов построено.
    pub stats_factories_built: i16,
    /// Сколько шахт построено.
    pub stats_mines_built: i16,
    /// Сколько зданий построено.
    pub stats_buildings_built: i16,
    /// Сколько мобильных юнитов построено.
    pub stats_units_built: i16,
    /// Потери по типам (V70 — u16[93], C++ — u32[93]).
    pub casualties: [u32; UNIT_COUNT],
    /// Золото, потраченное на апгрейды (V70 — i16, C++ — u32).
    pub stats_gold_spent_on_upgrades: u32,
}

impl Default for TeamInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl TeamInfo {
    /// Аналог `CTInfo::Reset`: пустой слот, счётчики имён = 1.
    pub fn new() -> Self {
        Self {
            team_type: TeamKind::None,
            finished_turn: false,
            team_clan: TeamClan::Random,
            research_topics: [ResearchTopicInfo::default(); RESEARCH_TOPIC_COUNT],
            team_points: 0,
            number_of_objects_created: 0,
            unit_counters: [1; UNIT_COUNT],
            score_graph: [0; SCORE_GRAPH_LEN],
            stats_factories_built: 0,
            stats_mines_built: 0,
            stats_buildings_built: 0,
            stats_units_built: 0,
            casualties: [0; UNIT_COUNT],
            stats_gold_spent_on_upgrades: 0,
        }
    }

    /// `CTInfo::Reset` — сброс в состояние пустого слота.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Счётчик `unit_counters` для типа (C++ — `unit_counters[UNIT_END]`).
    #[inline]
    pub fn unit_counter(&self, unit_type: u16) -> u32 {
        self.unit_counters[unit_type as usize]
    }

    /// Потери по типу (используются условиями победы/поражения).
    #[inline]
    pub fn casualties_for(&self, unit_type: u16) -> u32 {
        self.casualties[unit_type as usize]
    }
}

/// UI-поля `TeamInfo` из V70 (state-model §6.3 — в снапшот/симуляцию не входят).
///
/// Нужны только для побайтового round-trip сейва: в оригинале слепок экрана
/// попал в файл, потому что сохраняется весь процесс.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamInfoUi {
    /// Маркеры на карте (`markers[10]`), фича вырезана — всегда {-1,-1}.
    pub markers: [Point; MARKER_COUNT],
    /// Сохранённые позиции камеры F5–F8 (`camera_positions[6]`).
    pub screen_locations: [ScreenLocation; CAMERA_POSITION_COUNT],
    /// Выбранный юнит (`selected_unit`, 0xFFFF = нет).
    pub selected_unit: Option<u16>,
    /// Зум карты 4–64.
    pub zoom_level: u16,
    /// Центр экрана.
    pub camera_position: Point,
    /// 11 переключателей отображения (`display_button_*`).
    pub display_buttons: [i8; DISPLAY_BUTTON_COUNT],
}

impl Default for TeamInfoUi {
    fn default() -> Self {
        Self {
            markers: [Point::new(-1, -1); MARKER_COUNT],
            screen_locations: [ScreenLocation { x: -1, y: -1 }; CAMERA_POSITION_COUNT],
            selected_unit: None,
            zoom_level: 0,
            camera_position: Point::ZERO,
            display_buttons: [0; DISPLAY_BUTTON_COUNT],
        }
    }
}

// ---------------------------------------------------------------------------
// Комплексы и грузы
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// TeamUnits
// ---------------------------------------------------------------------------

/// Экономика и технологии команды — C++ `class TeamUnits` (`teamunits.hpp`).
///
/// В C++ это глобальные объекты `ResourceManager_TeamUnitsRed/Green/Blue/Gray`;
/// в `max_core` — элемент `GameState` по индексу команды.
///
/// `serde`-производные не выводятся из-за массивов на 93 элемента (см.
/// [`TeamInfo`]); байтовый формат — `save.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamUnits {
    /// Кредиты команды (C++ `uint32_t`, в V70 — i16/u16).
    pub gold: u32,
    /// Базовые атрибуты (с учётом клана) — 93 типа.
    pub base_values: [UnitValues; UNIT_COUNT],
    /// Текущие атрибуты (после исследований) — по ним создаются юниты.
    pub current_values: [UnitValues; UNIT_COUNT],
    /// Комплексы команды — порядок `SmartList` (по возрастанию id).
    pub complexes: Vec<Complex>,
    /// `hash_team_id` — служебное поле C++ (не сериализуется).
    pub hash_team_id: u16,
}

impl Default for TeamUnits {
    fn default() -> Self {
        Self::new()
    }
}

impl TeamUnits {
    /// Пустая команда: значения по умолчанию `UnitValues()`.
    pub fn new() -> Self {
        Self {
            gold: 0,
            base_values: [UnitValues::new(); UNIT_COUNT],
            current_values: [UnitValues::new(); UNIT_COUNT],
            complexes: Vec::new(),
            hash_team_id: 0,
        }
    }

    /// Команда, инициализированная таблицей атрибутов с учётом клана —
    /// C++ `TeamUnits::Init` + `ResourceManager_InitClanUnitValues`
    /// (base и current указывают на одинаковые значения до первых апгрейдов).
    pub fn with_clan(clan: TeamClan) -> Self {
        let values = crate::unit_values::team_base_values(clan);
        Self {
            gold: 0,
            base_values: values,
            current_values: values,
            complexes: Vec::new(),
            hash_team_id: 0,
        }
    }

    /// `TeamUnits::GetGold`.
    #[inline]
    pub const fn gold(&self) -> u32 {
        self.gold
    }

    /// `TeamUnits::SetGold`.
    #[inline]
    pub fn set_gold(&mut self, value: u32) {
        self.gold = value;
    }

    /// `TeamUnits::GetBaseUnitValues`.
    #[inline]
    pub fn base_unit_values(&self, unit_type: u16) -> &UnitValues {
        &self.base_values[unit_type as usize]
    }

    /// `TeamUnits::GetCurrentUnitValues`.
    #[inline]
    pub fn current_unit_values(&self, unit_type: u16) -> &UnitValues {
        &self.current_values[unit_type as usize]
    }

    /// `TeamUnits::GetBaseUnitValues` (mut).
    #[inline]
    pub fn base_unit_values_mut(&mut self, unit_type: u16) -> &mut UnitValues {
        &mut self.base_values[unit_type as usize]
    }

    /// `TeamUnits::GetCurrentUnitValues` (mut).
    #[inline]
    pub fn current_unit_values_mut(&mut self, unit_type: u16) -> &mut UnitValues {
        &mut self.current_values[unit_type as usize]
    }

    /// `TeamUnits::CreateComplex`: вставляет комплекс с минимальным свободным id,
    /// сохраняя сортировку списка по id (C++ `InsertBefore` первого разрыва).
    pub fn create_complex(&mut self) -> ComplexId {
        let mut complex_id: i16 = 1;
        let mut index = 0usize;
        while index < self.complexes.len() && self.complexes[index].id == complex_id {
            complex_id = complex_id.wrapping_add(1);
            index += 1;
        }
        let complex = Complex::new(complex_id);
        self.complexes.insert(index, complex);
        ComplexId(complex_id)
    }

    /// `TeamUnits::GetComplex`.
    pub fn get_complex(&self, complex_id: ComplexId) -> Option<&Complex> {
        self.complexes.iter().find(|item| item.id == complex_id.0)
    }

    /// `TeamUnits::GetComplex` (mut).
    pub fn get_complex_mut(&mut self, complex_id: ComplexId) -> Option<&mut Complex> {
        self.complexes.iter_mut().find(|item| item.id == complex_id.0)
    }

    /// `TeamUnits::GetComplexes`.
    #[inline]
    pub fn complexes(&self) -> &[Complex] {
        &self.complexes
    }

    /// `TeamUnits::RemoveComplex`.
    pub fn remove_complex(&mut self, complex_id: ComplexId) -> bool {
        let before = self.complexes.len();
        self.complexes.retain(|item| item.id != complex_id.0);
        self.complexes.len() != before
    }

    /// `TeamUnits::ClearComplexes`.
    pub fn clear_complexes(&mut self) {
        self.complexes.clear();
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

/// Количество типов юнитов для счётчиков команды (`UNIT_END`).
pub const TEAM_UNIT_TYPE_COUNT: u16 = UNIT_END;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::TeamClan;
    use crate::unit_values::base_unit_values;

    #[test]
    fn team_info_reset_matches_cpp() {
        let info = TeamInfo::new();
        assert_eq!(info.team_type, TeamKind::None);
        assert_eq!(info.team_clan, TeamClan::Random);
        assert!(info.unit_counters.iter().all(|&value| value == 1));
        assert!(info.casualties.iter().all(|&value| value == 0));
        assert_eq!(info.unit_counter(0), 1);
    }

    #[test]
    fn research_turns_uses_ceiling() {
        let mut topic = ResearchTopicInfo {
            research_level: 0,
            turns_to_complete: 7,
            allocation: 1,
        };
        assert_eq!(topic.turns_remaining(), 7);
        topic.allocation = 2;
        assert_eq!(topic.turns_remaining(), 4);
        topic.allocation = 7;
        assert_eq!(topic.turns_remaining(), 1);
        topic.allocation = 0;
        assert_eq!(topic.turns_remaining(), 0);
        assert!(topic.is_started());
        topic.turns_to_complete = 0;
        assert!(!topic.is_started());
    }

    #[test]
    fn complexes_are_created_in_id_order() {
        let mut units = TeamUnits::with_clan(TeamClan::TheChosen);
        assert_eq!(units.create_complex(), ComplexId(1));
        assert_eq!(units.create_complex(), ComplexId(2));
        assert_eq!(units.create_complex(), ComplexId(3));
        units.remove_complex(ComplexId(2));
        // C++ переиспользует минимальный свободный id.
        assert_eq!(units.create_complex(), ComplexId(2));
        let ids: Vec<i16> = units.complexes().iter().map(|complex| complex.id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
        assert!(units.get_complex(ComplexId(3)).is_some());
        units.clear_complexes();
        assert!(units.complexes().is_empty());
    }

    #[test]
    fn complex_grow_shrink() {
        let mut complex = Complex::new(1);
        complex.grow();
        complex.grow();
        assert_eq!(complex.buildings, 2);
        assert!(!complex.shrink());
        assert!(complex.shrink());
        assert_eq!(complex.buildings, 0);
    }

    #[test]
    fn team_units_initialized_from_clan_table() {
        let units = TeamUnits::with_clan(TeamClan::Musashi);
        // Musashi: Tank +4 attack, +2 armor, +4 hits, +1 speed.
        let tank = units.base_unit_values(crate::enums::UnitType::Tank as u16);
        let plain = base_unit_values(crate::enums::UnitType::Tank);
        assert_eq!(tank.attack, plain.attack + 4);
        assert_eq!(tank.armor, plain.armor + 2);
        assert_eq!(units.current_values, units.base_values);
        // Musashi: Gun Turret +4 attack, +2 armor, +4 hits.
        let turret = units.base_unit_values(crate::enums::UnitType::GunTurret as u16);
        let plain_turret = base_unit_values(crate::enums::UnitType::GunTurret);
        assert_eq!(turret.attack, plain_turret.attack + 4);
        assert_eq!(turret.armor, plain_turret.armor + 2);
        // Юнит, которого клан не касается, остаётся табличным.
        assert_eq!(
            units.base_unit_values(crate::enums::UnitType::Radar as u16),
            &base_unit_values(crate::enums::UnitType::Radar)
        );
        // Для пустого клана таблица совпадает с базовой.
        let random = TeamUnits::with_clan(TeamClan::Random);
        assert_eq!(random.base_values, crate::unit_values::BASE_UNIT_VALUES);
    }

    #[test]
    fn ui_defaults_match_v70_initialization() {
        let ui = TeamInfoUi::default();
        for marker in ui.markers {
            assert_eq!(marker, Point::new(-1, -1));
        }
        for location in ui.screen_locations {
            assert_eq!((location.x, location.y), (-1, -1));
        }
        assert_eq!(ui.selected_unit, None);
        assert_eq!(ui.display_buttons, [0; DISPLAY_BUTTON_COUNT]);
    }
}
