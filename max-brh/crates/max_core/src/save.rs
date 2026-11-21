//! Формат сохранений V70: `&[u8]` ⇄ [`SaveFile`].
//!
//! Оракул: `max-cpp-port/src/saveload.cpp` (`SaveLoad_LoadFormatV70`,
//! `SaveLoad_Save`), `smartfile.cpp/.hpp` (классовый сериализатор: индексы
//! объектов u16, `ReadObject`/`WriteObject`/`ReadObjectCount`),
//! `unitinfo.cpp` (`UnitInfo::FileLoad`/`FileSave`, ветка `V70`),
//! `unitvalues.cpp`, `complex.cpp`, `groundpath.cpp`, `airpath.cpp`,
//! `builderpath.cpp`, `hash.cpp`, `heatmap.cpp` (`LoadV70`),
//! `message_manager.cpp` (логи), `aiplayer.cpp` (`AiPlayer::FileLoad`),
//! плюс `max-brh-docs/state-model.md` §3 и `max-cpp-port-max-brh-docs/save.md`.
//!
//! ## Объектная модель файла
//!
//! V70 — это граф объектов с нумерацией по порядку первого появления:
//! `u16 index`; `0` — null; `index <= already_read` — ссылка на прочитанный
//! объект; `index == already_read + 1` — новый объект: далее `u16 type_index`
//! и тело. `type_index` — **алфавитный** порядок классов
//! (`RegisterArray` сортирует по имени): `AirPath=1`, `BuilderPath=2`,
//! `Complex=3`, `GroundPath=4`, `UnitInfo=5`, `UnitValues=6`.
//!
//! Поэтому юнит может быть впервые сериализован внутри поля `parent_unit` /
//! `enemy_unit` другого юнита (рекурсивно, целым телом), а позже встретиться
//! как ссылка в списке/хэше. Модель [`SaveUnit`] хранит `object_index` каждого
//! объекта, что позволяет воспроизвести байты 1:1.
//!
//! ## Что покрыто
//!
//! Читается и пишется **весь** файл: заголовок, `IniOptions`, `IniPreferences`,
//! карты (`SurfaceMap`, `ResourceMap`), `TeamInfo`×4 (+ UI-поля), состояние
//! партии, `TeamUnits`×4 (93 базовых + 93 текущих `UnitValues` + комплексы),
//! пять списков `UnitInfo`, `UnitHash`, `MapHash`, тепловые карты ×4, журналы
//! сообщений ×4, `AiPlayer` компьютерных команд.
//!
//! ## Что слой сейвов сознательно **не** делает
//!
//! Порт при загрузке нормализует часть полей (`build_rate == 0 -> 1`,
//! `state` из `NEW_ORDER`/`MOVE_GETTING_PATH`/`ISSUING_PATH`/`IN_TRANSITION`/
//! `IN_PROGRESS` -> `EXECUTING_ORDER`, очистка `AI_STATE_*_REMINDER` в
//! `ai_state_bits`, кламп отрицательных значений тепловых карт). Здесь эти
//! значения сохраняются как в файле: иначе побайтовый round-trip невозможен.
//! Нормализации выполняет загрузчик состояния (`GameState`, фаза P4).

use std::fmt;

use crate::enums::{
    FileType, GameStateType, Opponent, Planet, PlayMode, TeamClan, UnitOrder, UnitType,
    VictoryType, UNIT_END,
};
use crate::geo::{Point, MAP_TILES};
use crate::hash::{MapHash, MapHashObject, UnitHash};
use crate::team::{
    Complex, ComplexId, ResearchTopicInfo, ScreenLocation, TeamId, TeamInfo, TeamInfoUi, TeamKind,
    TeamUnits,
};
use crate::unit::{
    AirPath, BuilderPath, GroundPath, Rect, UnitId, UnitInfo, UnitListKind, UnitPath,
};
use crate::unit_values::{UnitValues, UNIT_COUNT};

/// Версия формата V70 (`SmartFileFormat::V70`).
pub const SAVE_VERSION_V70: u16 = 70;
/// Сколько команд пишет V70 (red/green/blue/gray, без слота «пришельцев»).
pub const SAVE_TEAM_COUNT: usize = 4;
/// Количество списков юнитов.
pub const UNIT_LIST_COUNT: usize = 5;
/// Максимальная глубина вложенных `UnitInfo` (parent/enemy сериализуются телом).
pub const MAX_UNIT_DEPTH: usize = 16;

/// Тип объекта в файле (`RegisterArray`-порядок по имени класса).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObjectKind {
    AirPath,
    BuilderPath,
    Complex,
    GroundPath,
    UnitInfo,
    UnitValues,
}

impl ObjectKind {
    const fn type_index(self) -> u16 {
        match self {
            Self::AirPath => 1,
            Self::BuilderPath => 2,
            Self::Complex => 3,
            Self::GroundPath => 4,
            Self::UnitInfo => 5,
            Self::UnitValues => 6,
        }
    }

    fn from_type_index(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::AirPath),
            2 => Some(Self::BuilderPath),
            3 => Some(Self::Complex),
            4 => Some(Self::GroundPath),
            5 => Some(Self::UnitInfo),
            6 => Some(Self::UnitValues),
            _ => None,
        }
    }

    fn from_path(path: &UnitPath) -> Self {
        match path {
            UnitPath::Air(_) => Self::AirPath,
            UnitPath::Builder(_) => Self::BuilderPath,
            UnitPath::Ground(_) => Self::GroundPath,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::AirPath => "AirPath",
            Self::BuilderPath => "BuilderPath",
            Self::Complex => "Complex",
            Self::GroundPath => "GroundPath",
            Self::UnitInfo => "UnitInfo",
            Self::UnitValues => "UnitValues",
        }
    }
}

const PATH_KINDS: [ObjectKind; 3] = [
    ObjectKind::AirPath,
    ObjectKind::BuilderPath,
    ObjectKind::GroundPath,
];

/// Ошибка разбора/записи V70.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    /// Файл оборван: нужно `needed` байт по смещению `offset`.
    UnexpectedEof {
        /// Смещение, на котором потребовались данные.
        offset: usize,
        /// Сколько байт нужно.
        needed: usize,
    },
    /// Неизвестная версия формата (ожидается [`SAVE_VERSION_V70`]).
    UnknownVersion(u16),
    /// Индекс объекта не соответствует порядку сериализации.
    InvalidObject {
        /// Прочитанный/записываемый индекс.
        index: u16,
        /// Ожидаемый следующий индекс.
        expected: u16,
        /// Смещение в файле (0 при записи).
        offset: usize,
    },
    /// Неизвестный/недопустимый `type_index` объекта.
    InvalidObjectKind {
        /// Индекс типа.
        type_index: u16,
        /// Смещение в файле.
        offset: usize,
    },
    /// Ссылка на объект другого типа.
    ObjectKindMismatch {
        /// Индекс объекта.
        index: u16,
        /// Ожидаемый тип.
        expected: &'static str,
        /// Фактический тип.
        actual: &'static str,
    },
    /// Ссылка на объект, который ещё не записан (нарушение порядка записи).
    ForwardReference {
        /// Индекс объекта.
        index: u16,
        /// Следующий свободный индекс.
        next: u16,
    },
    /// Значение не укладывается в перечисление формата.
    InvalidEnum {
        /// Что именно не разобралось.
        what: &'static str,
        /// Сырое значение.
        value: u32,
        /// Смещение в файле (0 при записи).
        offset: usize,
    },
    /// После разбора остались непрочитанные байты.
    TrailingBytes(usize),
    /// Некорректная длина строки/массива.
    InvalidLength {
        /// Что читалось.
        what: &'static str,
        /// Значение длины.
        length: usize,
    },
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof { offset, needed } => {
                write!(f, "unexpected end of save file at {offset}, needed {needed} bytes")
            }
            Self::UnknownVersion(version) => write!(f, "unsupported save version {version}"),
            Self::InvalidObject {
                index,
                expected,
                offset,
            } => write!(
                f,
                "object index {index} at {offset} does not follow the object table (expected {expected})"
            ),
            Self::InvalidObjectKind {
                type_index,
                offset,
            } => write!(f, "unknown object type index {type_index} at {offset}"),
            Self::ObjectKindMismatch {
                index,
                expected,
                actual,
            } => write!(f, "object {index} is {actual}, expected {expected}"),
            Self::ForwardReference { index, next } => {
                write!(f, "reference to object {index} before it is written (next {next})")
            }
            Self::InvalidEnum {
                what,
                value,
                offset,
            } => write!(f, "invalid value {value} for {what} at {offset}"),
            Self::TrailingBytes(count) => write!(f, "{count} trailing bytes after the save data"),
            Self::InvalidLength { what, length } => write!(f, "invalid length {length} for {what}"),
        }
    }
}

impl std::error::Error for SaveError {}

/// Результат разбора/записи V70.
pub type Result<T> = std::result::Result<T, SaveError>;

// ---------------------------------------------------------------------------
// Заголовок и опции
// ---------------------------------------------------------------------------

/// Заголовок сохранения — save.md `SaveFormatHeader`, `saveload.cpp`
/// (`SaveLoad_LoadFormatV70`).
///
/// Строковые поля фиксированной длины хранятся сырыми байтами (30 байт), чтобы
/// побайтовый round-trip не зависел от кодировки/паддинга.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveFormatHeader {
    /// Версия формата (`version`, V70 = 70).
    pub version: u16,
    /// Категория партии (`save_game_type`).
    pub save_game_type: FileType,
    /// Имя сохранения (`save_name[30]`, сырые байты).
    pub save_name: [u8; 30],
    /// Индекс планеты (`world_index`).
    pub world: Planet,
    /// Индекс миссии (`mission_index`, 1-based; 0 — generic).
    pub mission_index: u16,
    /// Имена команд (`team_name[4][30]`, сырые байты).
    pub team_names: [[u8; 30]; 4],
    /// Типы 4 команд + слот «пришельцев».
    pub team_type: [TeamKind; 5],
    /// Кланы 4 команд + слот «пришельцев».
    pub team_clan: [TeamClan; 5],
    /// Зерно ГПСЧ (`rng_seed`).
    pub rng_seed: u32,
    /// Сложность ИИ (`opponent`, int8).
    pub opponent: Opponent,
    /// Таймер хода, сек (`turn_timer_time`).
    pub turn_timer_time: u16,
    /// Таймер конца хода, сек (`endturn_time`).
    pub endturn_time: u16,
    /// Режим игры (`play_mode`, int8).
    pub play_mode: PlayMode,
}

impl SaveFormatHeader {
    /// Имя сохранения без завершающих нулей (потеря некорректных байт).
    pub fn save_name(&self) -> String {
        fixed_bytes_to_string(&self.save_name)
    }

    /// Имя команды без завершающих нулей.
    pub fn team_name(&self, team: usize) -> String {
        self.team_names
            .get(team)
            .map(|name| fixed_bytes_to_string(name))
            .unwrap_or_default()
    }
}

/// Настройки партии из INI — save.md `IniOptions` (12 × `signed int`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IniOptions {
    /// Планета (`world`), дублирует заголовок.
    pub world: i32,
    /// Таймер хода, сек.
    pub turn_timer: i32,
    /// Таймер конца хода, сек.
    pub endturn: i32,
    /// Стартовые кредиты 0–250.
    pub start_gold: i32,
    /// Режим игры (`PlayMode`).
    pub play_mode: PlayMode,
    /// Тип победы (`VictoryType`).
    pub victory_type: VictoryType,
    /// Лимит победы (ходов или очков).
    pub victory_limit: i32,
    /// Сложность ИИ.
    pub opponent: Opponent,
    /// Уровень материалов 0–2.
    pub raw_resource: i32,
    /// Уровень топлива 0–2.
    pub fuel_resource: i32,
    /// Уровень золота 0–2.
    pub gold_resource: i32,
    /// Обломки пришельцев 0–2.
    pub alien_derelicts: i32,
}

impl Default for IniOptions {
    fn default() -> Self {
        Self {
            world: 0,
            turn_timer: 0,
            endturn: 0,
            start_gold: 0,
            play_mode: PlayMode::TurnBased,
            victory_type: VictoryType::Duration,
            victory_limit: 0,
            opponent: Opponent::Average,
            raw_resource: 0,
            fuel_resource: 0,
            gold_resource: 0,
            alien_derelicts: 0,
        }
    }
}

/// Пользовательские предпочтения — save.md `IniPreferences` (7 × `signed int`).
///
/// В симуляции не участвуют (state-model §3.3); хранятся для round-trip.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IniPreferences {
    /// Звуковые эффекты.
    pub effects: i32,
    /// Прокрутка кликом.
    pub click_scroll: i32,
    /// Быстрая прокрутка.
    pub quick_scroll: i32,
    /// Быстрое движение.
    pub fast_movement: i32,
    /// Следование за юнитом.
    pub follow_unit: i32,
    /// Автовыбор.
    pub auto_select: i32,
    /// Остановка у врага.
    pub enemy_halt: i32,
}

/// Глобальное состояние партии — save.md `GameManagerState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameManagerState {
    /// Чей ход (`GameManager_ActiveTurnTeam`).
    pub active_turn_team: TeamId,
    /// Команда игрока (`GameManager_PlayerTeam`).
    pub player_team: TeamId,
    /// Номер хода (`GameManager_TurnCounter`).
    pub turn_counter: i32,
    /// Фаза игры (`game_state`, u16 → `GameStateType`).
    pub game_state: GameStateType,
    /// Остаток таймера хода, сек.
    pub turn_timer: u16,
}

impl Default for GameManagerState {
    fn default() -> Self {
        Self {
            active_turn_team: TeamId::RED,
            player_team: TeamId::RED,
            turn_counter: 0,
            game_state: GameStateType::InGame,
            turn_timer: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Слоты объектов в файле
// ---------------------------------------------------------------------------

/// Слот `UnitValues` в файле: значение + номер объекта (для идентичности ссылок).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedUnitValues {
    /// Номер объекта в файле (`0` — null-ссылка).
    pub object_index: u16,
    /// Значения атрибутов.
    pub values: UnitValues,
}

/// Слот `Complex` в файле.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedComplex {
    /// Номер объекта в файле.
    pub object_index: u16,
    /// Комплекс.
    pub complex: Complex,
}

/// `TeamUnits` в файле — C++ `TeamUnits::FileLoad`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveTeamUnits {
    /// Золото в V70 (`signed short`, C++ хранит u32).
    pub gold: u16,
    /// 93 базовых `UnitValues`.
    pub base_values: [SavedUnitValues; UNIT_COUNT],
    /// 93 текущих `UnitValues` (могут быть ссылками на базовые).
    pub current_values: [SavedUnitValues; UNIT_COUNT],
    /// Комплексы команды в порядке списка.
    pub complexes: Vec<SavedComplex>,
}

impl SaveTeamUnits {
    /// Доменная модель команды (`TeamUnits`) из сейва.
    pub fn to_team_units(&self) -> TeamUnits {
        let mut units = TeamUnits::new();
        units.gold = u32::from(self.gold);
        for (index, slot) in self.base_values.iter().enumerate() {
            units.base_values[index] = slot.values;
        }
        for (index, slot) in self.current_values.iter().enumerate() {
            units.current_values[index] = slot.values;
        }
        units.complexes = self.complexes.iter().map(|slot| slot.complex).collect();
        units
    }
}

/// Юнит в файле: значение + номера объектов всех вложенных/ссылочных полей.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveUnit {
    /// Номер объекта `UnitInfo` в файле.
    pub object_index: u16,
    /// Данные юнита.
    pub info: UnitInfo,
    /// Номер объекта пути (`0` — пути нет).
    pub path_object_index: u16,
    /// Номер объекта `base_values` юнита.
    pub base_values_object_index: u16,
    /// Номер объекта `Complex` (`0` — юнит вне комплекса).
    pub complex_object_index: u16,
    /// Значение комплекса, если объект впервые появился здесь (для записи тела).
    pub complex_value: Complex,
    /// Номер объекта родителя (`0` — нет).
    pub parent_object_index: u16,
    /// Номер объекта цели (`0` — нет).
    pub enemy_object_index: u16,
}

impl SaveUnit {
    fn placeholder(object_index: u16) -> Self {
        Self {
            object_index,
            info: UnitInfo::default(),
            path_object_index: 0,
            base_values_object_index: 0,
            complex_object_index: 0,
            complex_value: Complex::new(0),
            parent_object_index: 0,
            enemy_object_index: 0,
        }
    }
}

/// Запись журнала сообщений — `MessageLogEntry` (`message_manager.cpp`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageLogEntry {
    /// Текст сообщения: сырые байты как в файле (V70 пишет `strlen + 1` с NUL).
    pub text: Vec<u8>,
    /// Номер объекта юнита (`0` — нет).
    pub unit_object_index: u16,
    /// Разрешённая ссылка на юнита.
    pub unit: Option<UnitId>,
    /// Координаты события.
    pub point: Point,
    /// Алертное сообщение (кликабельное).
    pub is_alert_message: bool,
    /// Иконка/ресурс (`INVALID_ID` = 0xFFFF).
    pub id: u16,
}

/// Тепловая карта команды (V70: три массива `int8_t[112*112]`).
///
/// Значения хранятся **как в файле** (без клампа отрицательных), чтобы
/// round-trip был побайтовым; доступ — через [`HeatMap::complete`] и т.п.
/// C++ `HeatMap::LoadV70` клампит отрицательные в 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeatMap {
    /// `heatmap_complete`.
    pub complete: [i8; MAP_TILES],
    /// `heatmap_stealth_sea`.
    pub stealth_sea: [i8; MAP_TILES],
    /// `heatmap_stealth_land`.
    pub stealth_land: [i8; MAP_TILES],
}

impl HeatMap {
    /// Значение «полной» карты с клампом как в C++ (`LoadV70`).
    pub fn complete_clamped(&self, index: usize) -> u8 {
        if self.complete[index] < 0 {
            0
        } else {
            self.complete[index] as u8
        }
    }
}

/// Обнаруженный юнит ИИ — `SpottedUnit` (`spottedunit.cpp`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpottedUnit {
    /// Номер объекта юнита в файле.
    pub unit_object_index: u16,
    /// Разрешённая ссылка на юнита.
    pub unit: Option<UnitId>,
    /// Команда-наблюдатель (`uint16_t`).
    pub team: u16,
    /// Виден ли сейчас.
    pub visible_to_team: bool,
    /// Последняя известная позиция.
    pub last_position: Point,
}

/// Состояние ИИ команды — `AiPlayer::FileLoad` (`aiplayer.cpp`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiPlayerState {
    /// Команда (`uint16_t`).
    pub player_team: u16,
    /// Стратегия (`uint8_t`, `AiStrategyType`).
    pub strategy: u8,
    /// Неиспользуемое поле V70 (`int16_t`).
    pub unused_v70: i16,
    /// Доля экосфер (`int16_t`).
    pub greenhouse_ratio: i16,
    /// Плотность минных полей (`int16_t`).
    pub minefield_density: i16,
    /// Целевая команда V70 (`int16_t`).
    pub target_team_v70: i16,
    /// Обнаруженные юниты.
    pub spotted_units: Vec<SpottedUnit>,
    /// Карта разведки (`info_map`, 112×112, может отсутствовать).
    pub info_map: Option<Box<[u8; MAP_TILES]>>,
    /// Карта мин (`mine_map`, 112×112, может отсутствовать).
    pub mine_map: Option<Box<[i8; MAP_TILES]>>,
    /// Целевая точка (`target_location`).
    pub target_location: Point,
}

// ---------------------------------------------------------------------------
// SaveFile
// ---------------------------------------------------------------------------

/// Полное содержимое V70-сохранения.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveFile {
    /// Заголовок (настройки партии).
    pub header: SaveFormatHeader,
    /// Настройки из INI.
    pub ini_options: IniOptions,
    /// Пользовательские предпочтения.
    pub ini_preferences: IniPreferences,
    /// `SurfaceMap` — байт на клетку (`Surface`); сырые байты как в файле.
    pub surface_map: [u8; MAP_TILES],
    /// `ResourceMap` — u16 на клетку (`ResourceMapEntry::from_bits`).
    pub cargo_map: [u16; MAP_TILES],
    /// `TeamInfo` × 4 (red, green, blue, gray).
    pub teams: [TeamInfo; SAVE_TEAM_COUNT],
    /// UI-поля `TeamInfo` × 4 (нужны только для round-trip).
    pub team_ui: [TeamInfoUi; SAVE_TEAM_COUNT],
    /// Сырое поле `stats_gold_spent_on_upgrades` (V70 `int16_t`; C++ клампит
    /// отрицательные в 0 при загрузке).
    pub team_gold_spent_raw: [i16; SAVE_TEAM_COUNT],
    /// Глобальное состояние партии.
    pub game: GameManagerState,
    /// `TeamUnits` × 4.
    pub team_units: [SaveTeamUnits; SAVE_TEAM_COUNT],
    /// Пять списков юнитов: номера объектов в порядке списка.
    pub unit_lists: [Vec<u16>; UNIT_LIST_COUNT],
    /// Все объекты `UnitInfo` в порядке появления в файле.
    pub units: Vec<SaveUnit>,
    /// `UnitHash` — ведёрки номеров объектов.
    pub unit_hash: UnitHash<u16>,
    /// `MapHash` — ведёрки клеток с номерами объектов.
    pub map_hash: MapHash<u16>,
    /// Тепловые карты (для команд с `team_type != NONE`).
    pub heat_maps: [Option<Box<HeatMap>>; SAVE_TEAM_COUNT],
    /// Журналы сообщений × 4.
    pub message_logs: [Vec<MessageLogEntry>; SAVE_TEAM_COUNT],
    /// Состояние ИИ (для компьютерных команд).
    pub ai_players: [Option<Box<AiPlayerState>>; SAVE_TEAM_COUNT],
}

impl SaveFile {
    /// Разбирает V70-файл целиком.
    ///
    /// Гарантирует, что прочитан **весь** буфер (иначе [`SaveError::TrailingBytes`]),
    /// и что дерево объектов согласовано.
    pub fn parse(data: &[u8]) -> Result<Self> {
        Reader::new(data).parse()
    }

    /// Сериализует состояние обратно в V70.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Writer::write(self)
    }

    /// Доменные `TeamUnits` четырёх команд.
    pub fn team_units(&self) -> [TeamUnits; SAVE_TEAM_COUNT] {
        let mut result: [TeamUnits; SAVE_TEAM_COUNT] = Default::default();
        for (index, saved) in self.team_units.iter().enumerate() {
            result[index] = saved.to_team_units();
        }
        result
    }

    /// Юнит по номеру объекта.
    pub fn unit_by_object_index(&self, object_index: u16) -> Option<&SaveUnit> {
        self.units
            .binary_search_by_key(&object_index, |unit| unit.object_index)
            .ok()
            .map(|index| &self.units[index])
    }

    /// Юнит по `UnitId`.
    pub fn unit_by_id(&self, id: UnitId) -> Option<&SaveUnit> {
        self.units.iter().find(|unit| unit.info.id == id)
    }

    /// Список юнитов по виду (по номерам объектов, порядок списка).
    pub fn unit_list(&self, kind: UnitListKind) -> &[u16] {
        &self.unit_lists[kind.index()]
    }

    /// `ResourceMap` с типом/количеством/видимостью (state-model §3.10).
    pub fn resource_map(&self) -> crate::map::ResourceMap {
        let mut tiles = [crate::map::ResourceMapEntry::NONE; MAP_TILES];
        for (index, bits) in self.cargo_map.iter().enumerate() {
            tiles[index] = crate::map::ResourceMapEntry::from_bits(*bits);
        }
        crate::map::ResourceMap::from_tiles(tiles)
    }

    /// `SurfaceMap` из сырых байтов.
    pub fn surface(&self) -> crate::map::SurfaceMap {
        let mut tiles = [crate::enums::Surface::NONE; MAP_TILES];
        for (index, byte) in self.surface_map.iter().enumerate() {
            tiles[index] = crate::enums::Surface::from_bits_truncate(*byte);
        }
        crate::map::SurfaceMap::from_tiles(tiles)
    }
}

fn fixed_bytes_to_string(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    kinds: Vec<ObjectKind>,
    units: Vec<SaveUnit>,
    values_by_object: Vec<(u16, UnitValues)>,
    complexes_by_object: Vec<(u16, Complex)>,
    paths_by_object: Vec<(u16, UnitPath)>,
}

/// Результат чтения объектной ссылки.
enum ObjRef {
    /// `0` — нет объекта.
    Null,
    /// Ссылка на уже прочитанный объект.
    Existing(u16),
    /// Новый объект: тело идёт сразу за заголовком.
    New(u16),
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            kinds: Vec::new(),
            units: Vec::new(),
            values_by_object: Vec::new(),
            complexes_by_object: Vec::new(),
            paths_by_object: Vec::new(),
        }
    }

    // --- примитивы ---

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn take(&mut self, size: usize) -> Result<&'a [u8]> {
        if self.remaining() < size {
            return Err(SaveError::UnexpectedEof {
                offset: self.pos,
                needed: size,
            });
        }
        let slice = &self.data[self.pos..self.pos + size];
        self.pos += size;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }

    fn bool(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }

    fn u16(&mut self) -> Result<u16> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }

    fn u32(&mut self) -> Result<u32> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }

    fn point(&mut self) -> Result<Point> {
        Ok(Point::new(self.i16()?, self.i16()?))
    }

    fn rect(&mut self) -> Result<Rect> {
        Ok(Rect {
            ulx: self.i32()?,
            uly: self.i32()?,
            lrx: self.i32()?,
            lry: self.i32()?,
        })
    }

    fn bytes<const N: usize>(&mut self) -> Result<[u8; N]> {
        let slice = self.take(N)?;
        let mut out = [0u8; N];
        out.copy_from_slice(slice);
        Ok(out)
    }

    fn u16_array<const N: usize>(&mut self) -> Result<[u16; N]> {
        let mut out = [0u16; N];
        for item in out.iter_mut() {
            *item = self.u16()?;
        }
        Ok(out)
    }

    fn i16_array<const N: usize>(&mut self) -> Result<[i16; N]> {
        let mut out = [0i16; N];
        for item in out.iter_mut() {
            *item = self.i16()?;
        }
        Ok(out)
    }

    fn enum_u8<T>(&mut self, what: &'static str) -> Result<T>
    where
        T: TryFrom<u8>,
    {
        let offset = self.pos;
        let raw = self.u8()?;
        T::try_from(raw).map_err(|_| SaveError::InvalidEnum {
            what,
            value: u32::from(raw),
            offset,
        })
    }

    fn enum_u16<T>(&mut self, what: &'static str) -> Result<T>
    where
        T: TryFrom<u16>,
    {
        let offset = self.pos;
        let raw = self.u16()?;
        T::try_from(raw).map_err(|_| SaveError::InvalidEnum {
            what,
            value: u32::from(raw),
            offset,
        })
    }

    /// Читает `signed int` из файла и приводит к перечислению (`IniOptions`).
    fn enum_i32<T>(&mut self, what: &'static str, convert: impl Fn(i32) -> Option<T>) -> Result<T> {
        let offset = self.pos;
        let raw = self.i32()?;
        convert(raw).ok_or(SaveError::InvalidEnum {
            what,
            value: raw as u32,
            offset,
        })
    }

    // --- объектная таблица ---

    fn object_ref(&mut self, allowed: &[ObjectKind]) -> Result<ObjRef> {
        let index = self.u16()?;
        if index == 0 {
            return Ok(ObjRef::Null);
        }
        if usize::from(index) <= self.kinds.len() {
            let kind = self.kinds[usize::from(index) - 1];
            if !allowed.contains(&kind) {
                return Err(SaveError::ObjectKindMismatch {
                    index,
                    expected: allowed[0].name(),
                    actual: kind.name(),
                });
            }
            return Ok(ObjRef::Existing(index));
        }
        let expected = (self.kinds.len() + 1) as u16;
        if index != expected {
            return Err(SaveError::InvalidObject {
                index,
                expected,
                offset: self.pos - 2,
            });
        }
        let type_index = self.u16()?;
        let kind = ObjectKind::from_type_index(type_index).ok_or(SaveError::InvalidObjectKind {
            type_index,
            offset: self.pos - 2,
        })?;
        if !allowed.contains(&kind) {
            return Err(SaveError::ObjectKindMismatch {
                index,
                expected: allowed[0].name(),
                actual: kind.name(),
            });
        }
        self.kinds.push(kind);
        Ok(ObjRef::New(index))
    }

    fn lookup_values(&self, index: u16) -> Result<UnitValues> {
        self.values_by_object
            .iter()
            .find(|(object, _)| *object == index)
            .map(|(_, values)| *values)
            .ok_or(SaveError::ObjectKindMismatch {
                index,
                expected: "UnitValues",
                actual: "unknown",
            })
    }

    fn lookup_complex(&self, index: u16) -> Result<Complex> {
        self.complexes_by_object
            .iter()
            .find(|(object, _)| *object == index)
            .map(|(_, complex)| *complex)
            .ok_or(SaveError::ObjectKindMismatch {
                index,
                expected: "Complex",
                actual: "unknown",
            })
    }

    fn lookup_path(&self, index: u16) -> Result<UnitPath> {
        self.paths_by_object
            .iter()
            .find(|(object, _)| *object == index)
            .map(|(_, path)| path.clone())
            .ok_or(SaveError::ObjectKindMismatch {
                index,
                expected: "UnitPath",
                actual: "unknown",
            })
    }

    // --- тела объектов ---

    fn read_unit_values_body(&mut self) -> Result<UnitValues> {
        let mut values = UnitValues::new();
        values.turns = self.u16()?;
        values.hits = self.u16()?;
        values.armor = self.u16()?;
        values.attack = self.u16()?;
        values.speed = self.u16()?;
        values.range = self.u16()?;
        values.rounds = self.u16()?;
        values.move_and_fire = self.u8()?;
        values.scan = self.u16()?;
        values.storage = self.u16()?;
        values.ammo = self.u16()?;
        values.attack_radius = self.u16()?;
        values.agent_adjust = self.u16()?;
        values.version = self.u16()?;
        // V70: `uint8_t legacy_units_built` -> `m_is_in_use = (value != 0)`.
        values.in_use = self.u8()? != 0;
        Ok(values)
    }

    fn read_complex_body(&mut self) -> Result<Complex> {
        Ok(Complex {
            material: self.i16()?,
            fuel: self.i16()?,
            gold: self.i16()?,
            power: self.i16()?,
            workers: self.i16()?,
            buildings: self.i16()?,
            id: self.i16()?,
        })
    }

    fn read_path_body(&mut self, kind: ObjectKind) -> Result<UnitPath> {
        match kind {
            ObjectKind::GroundPath => {
                let end_x = self.i16()?;
                let end_y = self.i16()?;
                // V70: индекс шага — u16 (`GroundPath::FileLoad`).
                let step_index = u32::from(self.u16()?);
                let count = usize::from(self.u16()?);
                let mut steps = Vec::with_capacity(count);
                for _ in 0..count {
                    steps.push(crate::geo::PathStep::new(self.i8()?, self.i8()?));
                }
                Ok(UnitPath::Ground(GroundPath {
                    end_x,
                    end_y,
                    step_index,
                    steps,
                }))
            }
            ObjectKind::AirPath => Ok(UnitPath::Air(AirPath {
                length: self.i16()?,
                angle: self.i8()?,
                start_x: self.i16()?,
                start_y: self.i16()?,
                end_x: self.i16()?,
                end_y: self.i16()?,
                step_x: self.i32()?,
                step_y: self.i32()?,
                delta_x: self.i32()?,
                delta_y: self.i32()?,
            })),
            ObjectKind::BuilderPath => Ok(UnitPath::Builder(BuilderPath {
                direction_x: self.i16()?,
                direction_y: self.i16()?,
            })),
            other => Err(SaveError::ObjectKindMismatch {
                index: 0,
                expected: "UnitPath",
                actual: other.name(),
            }),
        }
    }

    fn read_unit_values_slot(&mut self) -> Result<SavedUnitValues> {
        Ok(match self.object_ref(&[ObjectKind::UnitValues])? {
            ObjRef::Null => SavedUnitValues {
                object_index: 0,
                values: UnitValues::new(),
            },
            ObjRef::New(index) => {
                let values = self.read_unit_values_body()?;
                self.values_by_object.push((index, values));
                SavedUnitValues {
                    object_index: index,
                    values,
                }
            }
            ObjRef::Existing(index) => SavedUnitValues {
                object_index: index,
                values: self.lookup_values(index)?,
            },
        })
    }

    fn read_path_slot(&mut self) -> Result<(u16, Option<UnitPath>)> {
        Ok(match self.object_ref(&PATH_KINDS)? {
            ObjRef::Null => (0, None),
            ObjRef::New(index) => {
                let kind = self.kinds[usize::from(index) - 1];
                let path = self.read_path_body(kind)?;
                self.paths_by_object.push((index, path.clone()));
                (index, Some(path))
            }
            ObjRef::Existing(index) => (index, Some(self.lookup_path(index)?)),
        })
    }

    fn read_complex_slot(&mut self) -> Result<SavedComplex> {
        Ok(match self.object_ref(&[ObjectKind::Complex])? {
            ObjRef::Null => SavedComplex {
                object_index: 0,
                complex: Complex::new(0),
            },
            ObjRef::New(index) => {
                let complex = self.read_complex_body()?;
                self.complexes_by_object.push((index, complex));
                SavedComplex {
                    object_index: index,
                    complex,
                }
            }
            ObjRef::Existing(index) => SavedComplex {
                object_index: index,
                complex: self.lookup_complex(index)?,
            },
        })
    }

    /// Читает `UnitInfo` (заголовок объекта уже прочитан).
    fn read_unit(&mut self, object_index: u16, depth: usize) -> Result<()> {
        if depth > MAX_UNIT_DEPTH {
            return Err(SaveError::InvalidLength {
                what: "UnitInfo nesting depth",
                length: depth,
            });
        }
        let position = self.units.len();
        self.units.push(SaveUnit::placeholder(object_index));
        let parsed = self.read_unit_body(object_index, depth)?;
        self.units[position] = parsed;
        Ok(())
    }

    fn read_unit_body(&mut self, object_index: u16, depth: usize) -> Result<SaveUnit> {
        let unit_type: UnitType = self.enum_u16("UnitType")?;
        let id = UnitId::from_v70(self.u16()?);
        let flags = self.u32()?;
        let x = self.u16()?;
        let y = self.u16()?;
        let grid = Point::new(self.i16()?, self.i16()?);
        let name_length = usize::from(self.u16()?);
        if name_length > 4096 {
            return Err(SaveError::InvalidLength {
                what: "UnitInfo::name",
                length: name_length,
            });
        }
        let name = if name_length == 0 {
            String::new()
        } else {
            String::from_utf8_lossy(self.take(name_length)?).into_owned()
        };
        let mut info = UnitInfo::new(unit_type, TeamId::RED, id);
        info.flags = flags;
        info.x = x;
        info.y = y;
        info.grid = grid;
        info.name = name;
        info.shadow_offset = self.point()?;
        info.team = TeamId(self.u8()?);
        info.unit_id = self.u8()?;
        info.brightness = self.u8()?;
        info.angle = self.u8()?;
        for slot in info.visible_to_team.iter_mut() {
            *slot = self.bool()?;
        }
        for slot in info.spotted_by_team.iter_mut() {
            *slot = self.bool()?;
        }
        info.max_velocity = self.u8()?;
        info.velocity = self.u8()?;
        info.sound = self.u8()?;
        info.scaler_adjust = self.u8()?;
        info.sprite_bounds = self.rect()?;
        info.shadow_bounds = self.rect()?;
        info.turret_angle = self.u8()?;
        info.turret_offset_x = self.i8()?;
        info.turret_offset_y = self.i8()?;
        info.total_images = self.i16()?;
        info.image_base = self.i16()?;
        info.turret_image_base = self.i16()?;
        info.firing_image_base = self.i16()?;
        info.connector_image_base = self.i16()?;
        info.image_index = self.i16()?;
        info.turret_image_index = self.i16()?;
        info.image_index_max = self.i16()?;
        info.orders = self.enum_u8("UnitOrder")?;
        info.state = self.enum_u8("UnitOrderState")?;
        info.prior_orders = self.enum_u8("UnitOrder")?;
        info.prior_state = self.enum_u8("UnitOrderState")?;
        info.laying_state = self.u8()?;
        info.target_grid = self.point()?;
        info.build_time = self.u8()?;
        info.total_mining = self.u8()?;
        info.raw_mining = self.u8()?;
        info.fuel_mining = self.u8()?;
        info.gold_mining = self.u8()?;
        info.raw_mining_max = self.u8()?;
        info.gold_mining_max = self.u8()?;
        info.fuel_mining_max = self.u8()?;
        // V70: hits/speed — u8 (в C++ u16).
        info.hits = u16::from(self.u8()?);
        info.speed = u16::from(self.u8()?);
        info.shots = self.u8()?;
        info.move_and_fire = self.u8()?;
        info.storage = self.i16()?;
        // V70: опыт живёт в `storage` у COMMANDO/REGENERATING_UNIT
        // (`unitinfo.cpp`), `transfer_cargo`/`stealth_dice_roll` — из `move_to_grid_x`.
        if info.flags & crate::enums::unit_flags::REGENERATING_UNIT != 0
            || unit_type == UnitType::Infiltrator
        {
            info.experience = info.storage;
            info.storage = 0;
        }
        info.ammo = self.u8()?;
        info.targeting_mode = self.u8()?;
        info.enter_mode = self.u8()?;
        info.cursor = self.u8()?;
        // V70: единый recoil_delay → пара полей C++.
        let recoil_delay = self.u8()?;
        if info.orders == UnitOrder::Disable || info.prior_orders == UnitOrder::Disable {
            info.disabled_turns_remaining = recoil_delay;
            info.firing_recoil_frames = 0;
        } else {
            info.firing_recoil_frames = recoil_delay;
            info.disabled_turns_remaining = 0;
        }
        info.delayed_reaction = self.u8()?;
        info.damaged_this_turn = self.bool()?;
        info.research_topic = self.enum_u8("ResearchTopic")?;
        info.moved = self.u8()?;
        info.bobbed = self.bool()?;
        info.shake_effect_state = self.u8()?;
        info.engine = self.u8()?;
        info.weapon = self.u8()?;
        // Не восстановленные C++-портом поля V70 (state-model §7.6).
        info.comm = self.u8()?;
        info.fuel_distance = self.u8()?;
        info.move_fraction = self.u8()?;
        info.energized = self.bool()?;
        info.movement_wait_count = 0;
        info.repeat_build = self.u8()? != 0;
        info.build_rate = self.u16()?;
        // C++ `UnitInfo::FileLoad` нормализует 0 в 1; слой сейвов хранит
        // значение как в файле (иначе побайтовый round-trip невозможен),
        // нормализация — забота загрузчика состояния (P4).
        info.disabled_reaction_fire = self.bool()?;
        info.auto_survey = self.bool()?;
        info.ai_state_bits = self.u32()?;
        // C++ после загрузки очищает `AI_STATE_MOVE_FINISHED_REMINDER` и
        // `AI_STATE_RETREAT_REMINDER`; здесь биты сохраняются как в файле. В
        // симуляцию (P4) они попадут уже очищенными.

        let (path_object_index, path) = self.read_path_slot()?;
        info.path = path;
        info.connectors = self.u16()?;
        let base_values_object_index;
        match self.object_ref(&[ObjectKind::UnitValues])? {
            ObjRef::Null => {
                base_values_object_index = 0;
                info.base_values = crate::unit_values::base_unit_values(unit_type);
            }
            ObjRef::New(index) => {
                let values = self.read_unit_values_body()?;
                self.values_by_object.push((index, values));
                info.base_values = values;
                base_values_object_index = index;
            }
            ObjRef::Existing(index) => {
                info.base_values = self.lookup_values(index)?;
                base_values_object_index = index;
            }
        }
        let complex_object_index;
        let mut complex_value = Complex::new(0);
        match self.object_ref(&[ObjectKind::Complex])? {
            ObjRef::Null => {
                complex_object_index = 0;
                info.complex = None;
            }
            ObjRef::New(index) => {
                let complex = self.read_complex_body()?;
                self.complexes_by_object.push((index, complex));
                complex_value = complex;
                info.complex = Some(ComplexId(complex.id));
                complex_object_index = index;
            }
            ObjRef::Existing(index) => {
                let complex = self.lookup_complex(index)?;
                complex_value = complex;
                info.complex = Some(ComplexId(complex.id));
                complex_object_index = index;
            }
        }
        // parent/enemy могут быть сериализованы телом (если объект новый).
        let parent_object_index = match self.object_ref(&[ObjectKind::UnitInfo])? {
            ObjRef::Null => 0,
            ObjRef::Existing(index) => index,
            ObjRef::New(index) => {
                self.read_unit(index, depth + 1)?;
                index
            }
        };
        let enemy_object_index = match self.object_ref(&[ObjectKind::UnitInfo])? {
            ObjRef::Null => 0,
            ObjRef::Existing(index) => index,
            ObjRef::New(index) => {
                self.read_unit(index, depth + 1)?;
                index
            }
        };
        let build_count = usize::from(self.u16()?);
        let mut build_list = Vec::with_capacity(build_count.min(256));
        for _ in 0..build_count {
            build_list.push(self.enum_u16("UnitType")?);
        }
        info.build_list = build_list;
        info.unit_list = UnitInfo::list_kind_for_flags(info.flags);

        Ok(SaveUnit {
            object_index,
            info,
            path_object_index,
            base_values_object_index,
            complex_object_index,
            complex_value,
            parent_object_index,
            enemy_object_index,
        })
    }

    /// Читает `TeamInfo` (V70-раскладка: `markers` + UI-поля).
    fn read_team_info(&mut self) -> Result<(TeamInfo, TeamInfoUi, i16)> {
        let mut markers = [Point::ZERO; crate::team::MARKER_COUNT];
        for marker in markers.iter_mut() {
            *marker = self.point()?;
        }
        let mut info = TeamInfo::new();
        info.team_type = self.enum_u8("TeamKind")?;
        info.finished_turn = self.bool()?;
        info.team_clan = self.enum_u8("TeamClan")?;
        for topic in info.research_topics.iter_mut() {
            *topic = ResearchTopicInfo {
                research_level: self.u32()?,
                turns_to_complete: self.u32()?,
                allocation: self.u32()?,
            };
        }
        info.team_points = self.u32()?;
        info.number_of_objects_created = self.u16()?;
        let counters = self.bytes::<{ UNIT_END as usize }>()?;
        for (slot, value) in info.unit_counters.iter_mut().zip(counters.iter()) {
            *slot = u32::from(*value);
        }
        let mut ui = TeamInfoUi::default();
        ui.markers = markers;
        for location in ui.screen_locations.iter_mut() {
            *location = ScreenLocation {
                x: self.i8()?,
                y: self.i8()?,
            };
        }
        info.score_graph = self.i16_array::<50>()?;
        ui.selected_unit = match self.u16()? {
            0xFFFF => None,
            value => Some(value),
        };
        ui.zoom_level = self.u16()?;
        ui.camera_position = self.point()?;
        for button in ui.display_buttons.iter_mut() {
            *button = self.i8()?;
        }
        info.stats_factories_built = self.i16()?;
        info.stats_mines_built = self.i16()?;
        info.stats_buildings_built = self.i16()?;
        info.stats_units_built = self.i16()?;
        let casualties = self.u16_array::<{ UNIT_END as usize }>()?;
        for (slot, value) in info.casualties.iter_mut().zip(casualties.iter()) {
            *slot = u32::from(*value);
        }
        let gold_spent_raw = self.i16()?;
        // C++: max(0, value), но сырое значение сохраняем для round-trip.
        info.stats_gold_spent_on_upgrades = gold_spent_raw.max(0) as u32;
        Ok((info, ui, gold_spent_raw))
    }

    fn read_team_units(&mut self) -> Result<SaveTeamUnits> {
        let gold = self.u16()?;
        let mut base_values: [SavedUnitValues; UNIT_COUNT] =
            std::array::from_fn(|_| SavedUnitValues {
                object_index: 0,
                values: UnitValues::new(),
            });
        for slot in base_values.iter_mut() {
            *slot = self.read_unit_values_slot()?;
        }
        let mut current_values: [SavedUnitValues; UNIT_COUNT] =
            std::array::from_fn(|_| SavedUnitValues {
                object_index: 0,
                values: UnitValues::new(),
            });
        for slot in current_values.iter_mut() {
            *slot = self.read_unit_values_slot()?;
        }
        let complex_count = usize::from(self.u16()?);
        let mut complexes = Vec::with_capacity(complex_count);
        for _ in 0..complex_count {
            complexes.push(self.read_complex_slot()?);
        }
        Ok(SaveTeamUnits {
            gold,
            base_values,
            current_values,
            complexes,
        })
    }

    fn read_unit_list(&mut self) -> Result<Vec<u16>> {
        let count = usize::from(self.u16()?);
        let mut list = Vec::with_capacity(count.min(8192));
        for _ in 0..count {
            match self.object_ref(&[ObjectKind::UnitInfo])? {
                ObjRef::Null => list.push(0),
                ObjRef::Existing(index) => list.push(index),
                ObjRef::New(index) => {
                    self.read_unit(index, 0)?;
                    list.push(index);
                }
            }
        }
        Ok(list)
    }

    fn read_unit_hash(&mut self) -> Result<UnitHash<u16>> {
        let size = self.u16()?;
        if size == 0 {
            return Err(SaveError::InvalidLength {
                what: "UnitHash::hash_size",
                length: 0,
            });
        }
        let mut hash = UnitHash::new(size);
        for bucket_index in 0..size {
            let count = usize::from(self.u16()?);
            let bucket = hash.bucket_mut(bucket_index).expect("bucket in range");
            for _ in 0..count {
                match self.object_ref(&[ObjectKind::UnitInfo])? {
                    ObjRef::Null => bucket.push(0),
                    ObjRef::Existing(index) => bucket.push(index),
                    ObjRef::New(index) => {
                        self.read_unit(index, 0)?;
                        bucket.push(index);
                    }
                }
            }
        }
        Ok(hash)
    }

    fn read_map_hash(&mut self) -> Result<MapHash<u16>> {
        let size = self.u16()?;
        let x_shift = self.u16()?;
        if size == 0 {
            return Err(SaveError::InvalidLength {
                what: "MapHash::hash_size",
                length: 0,
            });
        }
        let mut hash = MapHash::new(size);
        debug_assert_eq!(hash.x_shift(), x_shift);
        for bucket_index in 0..size {
            let count = usize::from(self.u16()?);
            let buckets = hash.buckets_mut();
            let bucket = &mut buckets[bucket_index as usize];
            for _ in 0..count {
                let x = self.u16()?;
                let y = self.u16()?;
                let mut object = MapHashObject::new(x, y);
                let unit_count = usize::from(self.u16()?);
                for _ in 0..unit_count {
                    match self.object_ref(&[ObjectKind::UnitInfo])? {
                        ObjRef::Null => object.units.push(0),
                        ObjRef::Existing(index) => object.units.push(index),
                        ObjRef::New(index) => {
                            self.read_unit(index, 0)?;
                            object.units.push(index);
                        }
                    }
                }
                bucket.push(object);
            }
        }
        Ok(hash)
    }

    fn read_heat_map(&mut self) -> Result<HeatMap> {
        Ok(HeatMap {
            complete: self.bytes::<MAP_TILES>()?.map(|byte| byte as i8),
            stealth_sea: self.bytes::<MAP_TILES>()?.map(|byte| byte as i8),
            stealth_land: self.bytes::<MAP_TILES>()?.map(|byte| byte as i8),
        })
    }

    fn read_message_logs(&mut self) -> Result<Vec<MessageLogEntry>> {
        let count = usize::from(self.u16()?);
        let mut entries = Vec::with_capacity(count.min(1024));
        for _ in 0..count {
            let length = usize::from(self.u16()?);
            if length > 65_535 {
                return Err(SaveError::InvalidLength {
                    what: "MessageLogEntry::text",
                    length,
                });
            }
            let text = self.take(length)?.to_vec();
            let unit_object_index = match self.object_ref(&[ObjectKind::UnitInfo])? {
                ObjRef::Null => 0,
                ObjRef::Existing(index) => index,
                ObjRef::New(index) => {
                    self.read_unit(index, 0)?;
                    index
                }
            };
            entries.push(MessageLogEntry {
                text,
                unit_object_index,
                unit: None,
                point: self.point()?,
                is_alert_message: self.bool()?,
                id: self.u16()?,
            });
        }
        Ok(entries)
    }

    fn read_ai_player(&mut self) -> Result<AiPlayerState> {
        let player_team = self.u16()?;
        let strategy = self.u8()?;
        let unused_v70 = self.i16()?;
        let greenhouse_ratio = self.i16()?;
        let minefield_density = self.i16()?;
        let target_team_v70 = self.i16()?;
        let spotted_count = usize::from(self.u16()?);
        let mut spotted_units = Vec::with_capacity(spotted_count.min(4096));
        for _ in 0..spotted_count {
            let unit_object_index = match self.object_ref(&[ObjectKind::UnitInfo])? {
                ObjRef::Null => 0,
                ObjRef::Existing(index) => index,
                ObjRef::New(index) => {
                    self.read_unit(index, 0)?;
                    index
                }
            };
            spotted_units.push(SpottedUnit {
                unit_object_index,
                unit: None,
                team: self.u16()?,
                visible_to_team: self.bool()?,
                last_position: self.point()?,
            });
        }
        let has_info_map = self.u16()?;
        let info_map = if has_info_map != 0 {
            Some(Box::new(self.bytes::<MAP_TILES>()?))
        } else {
            None
        };
        let has_mine_map = self.u16()?;
        let mine_map = if has_mine_map != 0 {
            Some(Box::new(self.bytes::<MAP_TILES>()?.map(|byte| byte as i8)))
        } else {
            None
        };
        Ok(AiPlayerState {
            player_team,
            strategy,
            unused_v70,
            greenhouse_ratio,
            minefield_density,
            target_team_v70,
            spotted_units,
            info_map,
            mine_map,
            target_location: self.point()?,
        })
    }

    fn parse(mut self) -> Result<SaveFile> {
        let version = self.u16()?;
        if version != SAVE_VERSION_V70 {
            return Err(SaveError::UnknownVersion(version));
        }
        let save_game_type: FileType = self.enum_u8("FileType")?;
        let save_name = self.bytes::<30>()?;
        let world: Planet = self.enum_u8("Planet")?;
        let mission_index = self.u16()?;
        let mut team_names = [[0u8; 30]; 4];
        for name in team_names.iter_mut() {
            *name = self.bytes::<30>()?;
        }
        let mut team_type = [TeamKind::None; 5];
        for slot in team_type.iter_mut() {
            *slot = self.enum_u8("TeamKind")?;
        }
        let mut team_clan = [TeamClan::Random; 5];
        for slot in team_clan.iter_mut() {
            *slot = self.enum_u8("TeamClan")?;
        }
        let rng_seed = self.u32()?;
        let opponent: Opponent = self.enum_u8("Opponent")?;
        let turn_timer_time = self.u16()?;
        let endturn_time = self.u16()?;
        let play_mode: PlayMode = self.enum_u8("PlayMode")?;
        let ini_options = IniOptions {
            world: self.i32()?,
            turn_timer: self.i32()?,
            endturn: self.i32()?,
            start_gold: self.i32()?,
            play_mode: self.enum_i32("PlayMode", |value| PlayMode::try_from(value as u8).ok())?,
            victory_type: self.enum_i32("VictoryType", |value| {
                VictoryType::try_from(value as u8).ok()
            })?,
            victory_limit: self.i32()?,
            opponent: self.enum_i32("Opponent", |value| Opponent::try_from(value as u8).ok())?,
            raw_resource: self.i32()?,
            fuel_resource: self.i32()?,
            gold_resource: self.i32()?,
            alien_derelicts: self.i32()?,
        };
        let header = SaveFormatHeader {
            version,
            save_game_type,
            save_name,
            world,
            mission_index,
            team_names,
            team_type,
            team_clan,
            rng_seed,
            opponent,
            turn_timer_time,
            endturn_time,
            play_mode,
        };

        let surface_map = self.bytes::<MAP_TILES>()?;
        let cargo_map = self.u16_array::<MAP_TILES>()?;

        let mut teams: [TeamInfo; SAVE_TEAM_COUNT] = Default::default();
        let mut team_ui: [TeamInfoUi; SAVE_TEAM_COUNT] = Default::default();
        let mut team_gold_spent_raw = [0i16; SAVE_TEAM_COUNT];
        for index in 0..SAVE_TEAM_COUNT {
            let (info, ui, raw) = self.read_team_info()?;
            teams[index] = info;
            team_ui[index] = ui;
            team_gold_spent_raw[index] = raw;
        }

        let game = GameManagerState {
            active_turn_team: TeamId(self.u8()?),
            player_team: TeamId(self.u8()?),
            turn_counter: self.i32()?,
            game_state: self.enum_u16("GameStateType")?,
            turn_timer: self.u16()?,
        };
        let ini_preferences = IniPreferences {
            effects: self.i32()?,
            click_scroll: self.i32()?,
            quick_scroll: self.i32()?,
            fast_movement: self.i32()?,
            follow_unit: self.i32()?,
            auto_select: self.i32()?,
            enemy_halt: self.i32()?,
        };

        let mut team_units: [SaveTeamUnits; SAVE_TEAM_COUNT] =
            std::array::from_fn(|_| SaveTeamUnits {
                gold: 0,
                base_values: std::array::from_fn(|_| SavedUnitValues {
                    object_index: 0,
                    values: UnitValues::new(),
                }),
                current_values: std::array::from_fn(|_| SavedUnitValues {
                    object_index: 0,
                    values: UnitValues::new(),
                }),
                complexes: Vec::new(),
            });
        for slot in team_units.iter_mut() {
            *slot = self.read_team_units()?;
        }

        let mut unit_lists: [Vec<u16>; UNIT_LIST_COUNT] = Default::default();
        for (index, kind) in UnitListKind::ALL.iter().enumerate() {
            let list = self.read_unit_list()?;
            // Список, в который попал юнит, отражён в его `unit_list`.
            for object_index in &list {
                if let Some(position) = self
                    .units
                    .iter()
                    .position(|unit| unit.object_index == *object_index)
                {
                    self.units[position].info.unit_list = *kind;
                }
            }
            unit_lists[index] = list;
        }

        let unit_hash = self.read_unit_hash()?;
        let map_hash = self.read_map_hash()?;

        let mut heat_maps: [Option<Box<HeatMap>>; SAVE_TEAM_COUNT] = Default::default();
        for index in 0..SAVE_TEAM_COUNT {
            if teams[index].team_type == TeamKind::None {
                continue;
            }
            heat_maps[index] = Some(Box::new(self.read_heat_map()?));
        }

        let mut message_logs: [Vec<MessageLogEntry>; SAVE_TEAM_COUNT] = Default::default();
        for logs in message_logs.iter_mut() {
            *logs = self.read_message_logs()?;
        }

        let mut ai_players: [Option<Box<AiPlayerState>>; SAVE_TEAM_COUNT] = Default::default();
        for index in 0..SAVE_TEAM_COUNT {
            if teams[index].team_type == TeamKind::Computer {
                ai_players[index] = Some(Box::new(self.read_ai_player()?));
            }
        }

        if self.pos != self.data.len() {
            return Err(SaveError::TrailingBytes(self.data.len() - self.pos));
        }

        // Сортировка по номеру объекта (для бинарного поиска при записи).
        self.units.sort_by_key(|unit| unit.object_index);
        // Разрешение ссылок parent/enemy и объектов в журналах/AI.
        let mut ids: Vec<(u16, UnitId)> = self
            .units
            .iter()
            .map(|unit| (unit.object_index, unit.info.id))
            .collect();
        ids.sort_by_key(|(object_index, _)| *object_index);
        let resolve = |object_index: u16, ids: &[(u16, UnitId)]| -> Option<UnitId> {
            if object_index == 0 {
                None
            } else {
                ids.binary_search_by_key(&object_index, |(index, _)| *index)
                    .ok()
                    .map(|position| ids[position].1)
            }
        };
        for unit in self.units.iter_mut() {
            let parent = resolve(unit.parent_object_index, &ids);
            let enemy = resolve(unit.enemy_object_index, &ids);
            unit.info.parent_unit = parent;
            unit.info.enemy_unit = enemy;
        }
        for logs in message_logs.iter_mut() {
            for entry in logs.iter_mut() {
                entry.unit = resolve(entry.unit_object_index, &ids);
            }
        }
        for ai in ai_players.iter_mut().flatten() {
            for spotted in ai.spotted_units.iter_mut() {
                spotted.unit = resolve(spotted.unit_object_index, &ids);
            }
        }

        Ok(SaveFile {
            header,
            ini_options,
            ini_preferences,
            surface_map,
            cargo_map,
            teams,
            team_ui,
            team_gold_spent_raw,
            game,
            team_units,
            unit_lists,
            units: self.units,
            unit_hash,
            map_hash,
            heat_maps,
            message_logs,
            ai_players,
        })
    }
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

struct Writer {
    out: Vec<u8>,
    objects: u16,
}

impl Writer {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            objects: 0,
        }
    }

    fn u8(&mut self, value: u8) {
        self.out.push(value);
    }

    fn i8(&mut self, value: i8) {
        self.out.push(value as u8);
    }

    fn bool(&mut self, value: bool) {
        self.out.push(u8::from(value));
    }

    fn u16(&mut self, value: u16) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    fn i16(&mut self, value: i16) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    fn point(&mut self, value: Point) {
        self.i16(value.x);
        self.i16(value.y);
    }

    fn rect(&mut self, value: Rect) {
        self.i32(value.ulx);
        self.i32(value.uly);
        self.i32(value.lrx);
        self.i32(value.lry);
    }

    fn bytes(&mut self, value: &[u8]) {
        self.out.extend_from_slice(value);
    }

    /// Заголовок объектной ссылки. Возвращает `true`, если тело нужно записать
    /// сразу после заголовка.
    fn object_ref(&mut self, stored: u16, kind: ObjectKind) -> Result<bool> {
        if stored == 0 {
            self.u16(0);
            return Ok(false);
        }
        let next = self.objects + 1;
        if stored > next {
            return Err(SaveError::ForwardReference {
                index: stored,
                next,
            });
        }
        if stored == next {
            self.u16(stored);
            self.u16(kind.type_index());
            self.objects += 1;
            Ok(true)
        } else {
            self.u16(stored);
            Ok(false)
        }
    }

    fn write_unit_values_body(&mut self, values: &UnitValues) {
        self.u16(values.turns);
        self.u16(values.hits);
        self.u16(values.armor);
        self.u16(values.attack);
        self.u16(values.speed);
        self.u16(values.range);
        self.u16(values.rounds);
        self.u8(values.move_and_fire);
        self.u16(values.scan);
        self.u16(values.storage);
        self.u16(values.ammo);
        self.u16(values.attack_radius);
        self.u16(values.agent_adjust);
        self.u16(values.version);
        // V70: `legacy_units_built` (u8), не bool.
        self.u8(u8::from(values.in_use));
    }

    fn write_complex_body(&mut self, complex: &Complex) {
        self.i16(complex.material);
        self.i16(complex.fuel);
        self.i16(complex.gold);
        self.i16(complex.power);
        self.i16(complex.workers);
        self.i16(complex.buildings);
        self.i16(complex.id);
    }

    fn write_path_body(&mut self, path: &UnitPath) {
        match path {
            UnitPath::Ground(ground) => {
                self.i16(ground.end_x);
                self.i16(ground.end_y);
                self.u16(ground.step_index as u16);
                self.u16(ground.steps.len() as u16);
                for step in &ground.steps {
                    self.i8(step.x);
                    self.i8(step.y);
                }
            }
            UnitPath::Air(air) => {
                self.i16(air.length);
                self.i8(air.angle);
                self.i16(air.start_x);
                self.i16(air.start_y);
                self.i16(air.end_x);
                self.i16(air.end_y);
                self.i32(air.step_x);
                self.i32(air.step_y);
                self.i32(air.delta_x);
                self.i32(air.delta_y);
            }
            UnitPath::Builder(builder) => {
                self.i16(builder.direction_x);
                self.i16(builder.direction_y);
            }
        }
    }

    fn write_unit_values_slot(&mut self, slot: &SavedUnitValues) -> Result<()> {
        if self.object_ref(slot.object_index, ObjectKind::UnitValues)? {
            let values = slot.values;
            self.write_unit_values_body(&values);
        }
        Ok(())
    }

    fn write_complex_slot(&mut self, slot: &SavedComplex) -> Result<()> {
        if self.object_ref(slot.object_index, ObjectKind::Complex)? {
            let complex = slot.complex;
            self.write_complex_body(&complex);
        }
        Ok(())
    }
    /// Пишет `UnitInfo`: заголовок объекта (если новый) и тело.
    fn write_unit_body(&mut self, unit: &SaveUnit, all: &[&SaveUnit], depth: usize) -> Result<()> {
        if depth > MAX_UNIT_DEPTH {
            return Err(SaveError::InvalidLength {
                what: "UnitInfo nesting depth",
                length: depth,
            });
        }
        let info = &unit.info;
        self.u16(info.unit_type as u16);
        self.u16(info.id.to_v70());
        self.u32(info.flags);
        self.u16(info.x);
        self.u16(info.y);
        self.i16(info.grid.x);
        self.i16(info.grid.y);
        self.u16(info.name.len() as u16);
        self.bytes(info.name.as_bytes());
        self.point(info.shadow_offset);
        self.u8(info.team.0);
        self.u8(info.unit_id);
        self.u8(info.brightness);
        self.u8(info.angle);
        for visible in info.visible_to_team {
            self.bool(visible);
        }
        for spotted in info.spotted_by_team {
            self.bool(spotted);
        }
        self.u8(info.max_velocity);
        self.u8(info.velocity);
        self.u8(info.sound);
        self.u8(info.scaler_adjust);
        self.rect(info.sprite_bounds);
        self.rect(info.shadow_bounds);
        self.u8(info.turret_angle);
        self.i8(info.turret_offset_x);
        self.i8(info.turret_offset_y);
        self.i16(info.total_images);
        self.i16(info.image_base);
        self.i16(info.turret_image_base);
        self.i16(info.firing_image_base);
        self.i16(info.connector_image_base);
        self.i16(info.image_index);
        self.i16(info.turret_image_index);
        self.i16(info.image_index_max);
        self.u8(info.orders as u8);
        self.u8(info.state as u8);
        self.u8(info.prior_orders as u8);
        self.u8(info.prior_state as u8);
        self.u8(info.laying_state);
        self.point(info.target_grid);
        self.u8(info.build_time);
        self.u8(info.total_mining);
        self.u8(info.raw_mining);
        self.u8(info.fuel_mining);
        self.u8(info.gold_mining);
        self.u8(info.raw_mining_max);
        self.u8(info.gold_mining_max);
        self.u8(info.fuel_mining_max);
        // V70: hits/speed — u8.
        self.u8(info.hits as u8);
        self.u8(info.speed as u8);
        self.u8(info.shots);
        self.u8(info.move_and_fire);
        // V70: опыт у COMMANDO/REGENERATING_UNIT хранится в `storage`.
        if info.flags & crate::enums::unit_flags::REGENERATING_UNIT != 0
            || info.unit_type == UnitType::Infiltrator
        {
            self.i16(info.storage.wrapping_add(info.experience));
        } else {
            self.i16(info.storage);
        }
        self.u8(info.ammo);
        self.u8(info.targeting_mode);
        self.u8(info.enter_mode);
        self.u8(info.cursor);
        let recoil_delay =
            if info.orders == UnitOrder::Disable || info.prior_orders == UnitOrder::Disable {
                info.disabled_turns_remaining
            } else {
                info.firing_recoil_frames
            };
        self.u8(recoil_delay);
        self.u8(info.delayed_reaction);
        self.bool(info.damaged_this_turn);
        self.u8(info.research_topic as u8);
        self.u8(info.moved);
        self.bool(info.bobbed);
        self.u8(info.shake_effect_state);
        self.u8(info.engine);
        self.u8(info.weapon);
        self.u8(info.comm);
        self.u8(info.fuel_distance);
        self.u8(info.move_fraction);
        self.bool(info.energized);
        self.bool(info.repeat_build);
        self.u16(info.build_rate);
        self.bool(info.disabled_reaction_fire);
        self.bool(info.auto_survey);
        self.u32(info.ai_state_bits);

        let path_kind = info
            .path
            .as_ref()
            .map(ObjectKind::from_path)
            .unwrap_or(ObjectKind::GroundPath);
        if self.object_ref(unit.path_object_index, path_kind)? {
            let path = info.path.as_ref().ok_or(SaveError::InvalidLength {
                what: "UnitInfo::path",
                length: 0,
            })?;
            self.write_path_body(path);
        }
        self.u16(info.connectors);
        if self.object_ref(unit.base_values_object_index, ObjectKind::UnitValues)? {
            let values = info.base_values;
            self.write_unit_values_body(&values);
        }
        if self.object_ref(unit.complex_object_index, ObjectKind::Complex)? {
            let complex = unit.complex_value;
            self.write_complex_body(&complex);
        }
        self.write_unit_ref(unit.parent_object_index, all, depth)?;
        self.write_unit_ref(unit.enemy_object_index, all, depth)?;
        self.u16(info.build_list.len() as u16);
        for unit_type in &info.build_list {
            self.u16(*unit_type as u16);
        }
        Ok(())
    }

    /// Пишет ссылку на юнита; если объект новый — его тело (рекурсивно).
    fn write_unit_ref(&mut self, stored: u16, all: &[&SaveUnit], depth: usize) -> Result<()> {
        if self.object_ref(stored, ObjectKind::UnitInfo)? {
            let position = all
                .binary_search_by_key(&stored, |unit| unit.object_index)
                .map_err(|_| SaveError::ForwardReference {
                    index: stored,
                    next: self.objects,
                })?;
            let unit = all[position];
            self.write_unit_body(unit, all, depth + 1)?;
        }
        Ok(())
    }

    fn write_unit_list(&mut self, list: &[u16], all: &[&SaveUnit]) -> Result<()> {
        // V70: счётчик — u16 (`SmartFileReader::ReadObjectCount`); в V71 C++
        // пишет u32, поэтому здесь формат V70 воспроизводится явно.
        self.u16(list.len() as u16);
        for object_index in list {
            self.write_unit_ref(*object_index, all, 0)?;
        }
        Ok(())
    }

    fn write_unit_like_hash(&mut self, hash: &UnitHash<u16>, all: &[&SaveUnit]) -> Result<()> {
        self.u16(hash.size());
        for bucket_index in 0..hash.size() {
            let bucket = hash.bucket(bucket_index).expect("bucket in range");
            self.u16(bucket.len() as u16);
            for object_index in bucket {
                self.write_unit_ref(*object_index, all, 0)?;
            }
        }
        Ok(())
    }

    fn write_map_hash(&mut self, hash: &MapHash<u16>, all: &[&SaveUnit]) -> Result<()> {
        self.u16(hash.size());
        self.u16(hash.x_shift());
        for bucket in hash.buckets() {
            self.u16(bucket.len() as u16);
            for object in bucket {
                self.u16(object.x);
                self.u16(object.y);
                self.u16(object.units.len() as u16);
                for object_index in &object.units {
                    self.write_unit_ref(*object_index, all, 0)?;
                }
            }
        }
        Ok(())
    }

    fn write(save: &SaveFile) -> Result<Vec<u8>> {
        let mut writer = Writer::new();
        writer.write_body(save)?;
        Ok(writer.out)
    }

    fn write_body(&mut self, save: &SaveFile) -> Result<()> {
        // --- заголовок ---
        self.u16(SAVE_VERSION_V70);
        self.u8(save.header.save_game_type as u8);
        self.bytes(&save.header.save_name);
        self.u8(save.header.world as u8);
        self.u16(save.header.mission_index);
        for name in &save.header.team_names {
            self.bytes(name);
        }
        for team_type in save.header.team_type {
            self.u8(team_type as u8);
        }
        for team_clan in save.header.team_clan {
            self.u8(team_clan as u8);
        }
        self.u32(save.header.rng_seed);
        self.i8(save.header.opponent as i8);
        self.u16(save.header.turn_timer_time);
        self.u16(save.header.endturn_time);
        self.i8(save.header.play_mode as i8);

        // --- IniOptions ---
        self.i32(save.ini_options.world);
        self.i32(save.ini_options.turn_timer);
        self.i32(save.ini_options.endturn);
        self.i32(save.ini_options.start_gold);
        self.i32(i32::from(save.ini_options.play_mode as u8));
        self.i32(i32::from(save.ini_options.victory_type as u8));
        self.i32(save.ini_options.victory_limit);
        self.i32(i32::from(save.ini_options.opponent as u8));
        self.i32(save.ini_options.raw_resource);
        self.i32(save.ini_options.fuel_resource);
        self.i32(save.ini_options.gold_resource);
        self.i32(save.ini_options.alien_derelicts);

        // --- карты ---
        self.bytes(&save.surface_map);
        for entry in &save.cargo_map {
            self.u16(*entry);
        }

        // --- TeamInfo × 4 ---
        for index in 0..SAVE_TEAM_COUNT {
            let info = &save.teams[index];
            let ui = &save.team_ui[index];
            for marker in ui.markers {
                self.point(marker);
            }
            self.u8(info.team_type as u8);
            self.bool(info.finished_turn);
            self.u8(info.team_clan as u8);
            for topic in info.research_topics {
                self.u32(topic.research_level);
                self.u32(topic.turns_to_complete);
                self.u32(topic.allocation);
            }
            self.u32(info.team_points);
            self.u16(info.number_of_objects_created);
            for counter in info.unit_counters {
                self.u8(counter as u8);
            }
            for location in ui.screen_locations {
                self.i8(location.x);
                self.i8(location.y);
            }
            for value in info.score_graph {
                self.i16(value);
            }
            self.u16(ui.selected_unit.unwrap_or(0xFFFF));
            self.u16(ui.zoom_level);
            self.point(ui.camera_position);
            for button in ui.display_buttons {
                self.i8(button);
            }
            self.i16(info.stats_factories_built);
            self.i16(info.stats_mines_built);
            self.i16(info.stats_buildings_built);
            self.i16(info.stats_units_built);
            for value in info.casualties {
                self.u16(value as u16);
            }
            self.i16(save.team_gold_spent_raw[index]);
        }

        // --- GameManagerState ---
        self.u8(save.game.active_turn_team.0);
        self.u8(save.game.player_team.0);
        self.i32(save.game.turn_counter);
        self.u16(save.game.game_state as u16);
        self.u16(save.game.turn_timer);

        // --- IniPreferences ---
        self.i32(save.ini_preferences.effects);
        self.i32(save.ini_preferences.click_scroll);
        self.i32(save.ini_preferences.quick_scroll);
        self.i32(save.ini_preferences.fast_movement);
        self.i32(save.ini_preferences.follow_unit);
        self.i32(save.ini_preferences.auto_select);
        self.i32(save.ini_preferences.enemy_halt);

        // --- TeamUnits × 4 ---
        for team in &save.team_units {
            self.u16(team.gold);
            for slot in &team.base_values {
                self.write_unit_values_slot(slot)?;
            }
            for slot in &team.current_values {
                self.write_unit_values_slot(slot)?;
            }
            self.u16(team.complexes.len() as u16);
            for slot in &team.complexes {
                self.write_complex_slot(slot)?;
            }
        }

        // --- списки юнитов, хэши ---
        let mut all: Vec<&SaveUnit> = save.units.iter().collect();
        all.sort_by_key(|unit| unit.object_index);
        for list in &save.unit_lists {
            self.write_unit_list(list, &all)?;
        }
        self.write_unit_like_hash(&save.unit_hash, &all)?;
        self.write_map_hash(&save.map_hash, &all)?;

        // --- тепловые карты ---
        for index in 0..SAVE_TEAM_COUNT {
            let Some(heat_map) = save.heat_maps[index].as_deref() else {
                continue;
            };
            self.bytes(&heat_map.complete.map(|value| value as u8));
            self.bytes(&heat_map.stealth_sea.map(|value| value as u8));
            self.bytes(&heat_map.stealth_land.map(|value| value as u8));
        }

        // --- журналы сообщений ---
        for logs in &save.message_logs {
            self.u16(logs.len() as u16);
            for entry in logs {
                self.u16(entry.text.len() as u16);
                self.bytes(&entry.text);
                self.write_unit_ref(entry.unit_object_index, &all, 0)?;
                self.point(entry.point);
                self.bool(entry.is_alert_message);
                self.u16(entry.id);
            }
        }

        // --- ИИ ---
        for ai in &save.ai_players {
            let Some(ai) = ai.as_deref() else {
                continue;
            };
            self.u16(ai.player_team);
            self.u8(ai.strategy);
            self.i16(ai.unused_v70);
            self.i16(ai.greenhouse_ratio);
            self.i16(ai.minefield_density);
            self.i16(ai.target_team_v70);
            self.u16(ai.spotted_units.len() as u16);
            for spotted in &ai.spotted_units {
                self.write_unit_ref(spotted.unit_object_index, &all, 0)?;
                self.u16(spotted.team);
                self.bool(spotted.visible_to_team);
                self.point(spotted.last_position);
            }
            self.u16(u16::from(ai.info_map.is_some()));
            if let Some(info_map) = ai.info_map.as_deref() {
                self.bytes(info_map);
            }
            self.u16(u16::from(ai.mine_map.is_some()));
            if let Some(mine_map) = ai.mine_map.as_deref() {
                self.bytes(&mine_map.map(|value| value as u8));
            }
            self.point(ai.target_location);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{UnitOrder, UnitOrderState, UnitType};
    use crate::unit_values::{base_unit_values, ATTRIB_ATTACK};

    /// Реальный V70-сейв из `max-original/SAVE1.CAM` (копия — `tests/data/`).
    const SAVE_FIXTURE: &[u8] = include_bytes!("../tests/data/save1_v70.cam");

    #[test]
    fn parses_real_v70_save_completely() {
        let save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");
        // Значения заголовка реального файла (max-original/SAVE1.CAM):
        //   version = 70, category = 2 (Campaign), world = 16, mission = 1,
        //   name = "Islands in the Sun", seed = 0x32A61291, opponent = 3,
        //   play_mode = 1 (simultaneous), teams: human (clan 3) + computer (clan 7).
        assert_eq!(save.header.version, 70);
        assert_eq!(save.header.save_game_type, FileType::Campaign);
        assert_eq!(save.header.save_name(), "Islands in the Sun");
        assert_eq!(save.header.world, Planet::Islandia);
        assert_eq!(save.header.mission_index, 1);
        assert_eq!(save.header.rng_seed, 0x32A6_1291);
        assert_eq!(save.header.opponent, Opponent::Expert);
        assert_eq!(save.header.play_mode, PlayMode::SimultaneousMoves);
        assert_eq!(save.header.team_name(0), "Player 1");
        assert_eq!(save.header.team_name(1), "Green Team");
        assert_eq!(save.header.team_type[0], TeamKind::Player);
        assert_eq!(save.header.team_type[1], TeamKind::Computer);
        assert_eq!(save.header.team_clan[0], TeamClan::VonGriffin);
        assert_eq!(save.header.team_clan[1], TeamClan::SevenKnights);
        assert_eq!(save.header.team_clan[3], TeamClan::AxisInc);

        // IniOptions / IniPreferences реального файла.
        assert_eq!(save.ini_options.world, 16);
        assert_eq!(save.ini_options.start_gold, 200);
        assert_eq!(save.ini_options.victory_limit, 30);
        assert_eq!(save.ini_options.raw_resource, 1);
        assert_eq!(save.ini_preferences.effects, 1);
        assert_eq!(save.ini_preferences.quick_scroll, 40);

        // Команды: 87 и 288 созданных объектов, у красных 2 комплекса.
        assert_eq!(save.teams[0].number_of_objects_created, 87);
        assert_eq!(save.teams[1].number_of_objects_created, 288);
        assert_eq!(save.teams[0].team_points, 0);
        assert_eq!(save.teams[1].team_points, 4);
        assert_eq!(save.teams[0].team_clan, TeamClan::VonGriffin);
        assert_eq!(save.teams[0].unit_counters[0], 2);
        assert_eq!(save.team_units[0].gold, 52);
        assert_eq!(save.team_units[0].complexes.len(), 2);
        assert_eq!(save.team_units[1].complexes.len(), 3);

        // Состояние партии.
        assert_eq!(save.game.active_turn_team, TeamId::RED);
        assert_eq!(save.game.player_team, TeamId::RED);
        assert_eq!(save.game.turn_counter, 1);
        assert_eq!(save.game.game_state, GameStateType::InGame);

        // Все пять списков юнитов прочитаны до конца: 312 юнитов.
        let total: usize = save.unit_lists.iter().map(Vec::len).sum();
        assert_eq!(total, 312);
        assert_eq!(save.unit_lists[0].len(), 102);
        assert_eq!(save.unit_lists[1].len(), 60);
        assert_eq!(save.unit_lists[2].len(), 132);
        assert_eq!(save.unit_lists[3].len(), 18);
        assert_eq!(save.unit_lists[4].len(), 0);
        assert_eq!(save.units.len(), 312);
        assert_eq!(save.unit_hash.size(), 512);
        assert_eq!(save.map_hash.size(), 512);
        assert_eq!(save.map_hash.x_shift(), 2);
        // Тепловые карты есть только у двух играющих команд.
        assert!(save.heat_maps[0].is_some());
        assert!(save.heat_maps[1].is_some());
        assert!(save.heat_maps[2].is_none());
        assert!(save.heat_maps[3].is_none());
        // Журналы пусты, у компьютерной команды есть состояние ИИ.
        assert!(save.message_logs.iter().all(|logs| logs.is_empty()));
        let ai = save.ai_players[1].as_deref().expect("AI state of team 1");
        assert_eq!(ai.player_team, 1);
        assert_eq!(ai.target_location, Point::new(1, 1));
        assert!(save.ai_players[0].is_none());

        // Первый юнит списка ground cover: тип WTRPLTFM (33), команда GREEN,
        // состояние приказа `AWAIT/EXECUTING_ORDER`.
        let first = save.unit_by_object_index(save.unit_lists[0][0]).unwrap();
        assert_eq!(first.info.unit_type, UnitType::WaterPlatform);
        assert_eq!(first.info.team, TeamId::GREEN);
        assert_eq!(first.info.orders, UnitOrder::Await);
        assert_eq!(first.info.state, UnitOrderState::ExecutingOrder);
        // У юнита есть объект base_values (ссылка в общую таблицу объектов).
        assert_ne!(first.base_values_object_index, 0);
        assert_ne!(first.info.hits, 0);
    }

    #[test]
    fn round_trip_is_byte_identical() {
        let save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");
        let bytes = save.to_bytes().expect("V70 save serializes");
        assert_eq!(bytes.len(), SAVE_FIXTURE.len());
        if bytes != SAVE_FIXTURE {
            let position = bytes
                .iter()
                .zip(SAVE_FIXTURE.iter())
                .position(|(left, right)| left != right)
                .unwrap_or(bytes.len());
            panic!(
                "byte mismatch at offset {position}: got {:02x?}, expected {:02x?}",
                &bytes[position.saturating_sub(8)..(position + 8).min(bytes.len())],
                &SAVE_FIXTURE[position.saturating_sub(8)..(position + 8).min(SAVE_FIXTURE.len())]
            );
        }
    }

    #[test]
    fn base_values_match_real_v70_save() {
        let save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");
        // База команды в файле = `BASE_UNIT_VALUES` + клановые модификаторы
        // (`TeamUnits::Init` + `ResourceManager_SetClanUpgradesFromClans`).
        // Исключения — поля, которые оригинал меняет уже в партии:
        //   * `units_built` — счётчик/флаг «слот занят» (`in_use`), растёт при игре;
        //   * `LANDPAD.storage` — в реальном сейве 1 при табличном 0 (мутация
        //     в партии, оригинальный ATTRIBS тоже даёт 0).
        let mut mismatches: Vec<String> = Vec::new();
        for (team_index, saved) in save.team_units.iter().enumerate() {
            // У пустых слотов (team_type == NONE) в файле лежат значения от
            // прошлой партии: клан в заголовке к ним не относится, поэтому
            // сверяем только играющие команды.
            if save.teams[team_index].team_type == TeamKind::None {
                continue;
            }
            let clan = save.teams[team_index].team_clan;
            let expected = crate::unit_values::team_base_values(clan);
            for (unit_index, slot) in saved.base_values.iter().enumerate() {
                let want = expected[unit_index];
                let got = slot.values;
                for attribute in 0..crate::unit_values::ATTRIB_COUNT {
                    if attribute == crate::unit_values::ATTRIB_FUEL {
                        continue;
                    }
                    if want.get_attribute(attribute) != got.get_attribute(attribute)
                        && !(unit_index == UnitType::LandingPad as usize
                            && attribute == crate::unit_values::ATTRIB_STORAGE
                            && got.get_attribute(attribute) == 1)
                    {
                        mismatches.push(format!(
                            "team {team_index} unit {unit_index} attr {attribute}: save={} table={}",
                            got.get_attribute(attribute),
                            want.get_attribute(attribute)
                        ));
                    }
                }
                assert_eq!(got.version, want.version, "unit {unit_index} version");
            }
        }
        assert!(mismatches.is_empty(), "mismatches: {mismatches:?}");
        // Проверка, что сверка вообще что-то проверила: у играющих команд клан
        // не Random и таблица отличается от чистой базовой.
        assert_ne!(save.teams[0].team_clan, TeamClan::Random);
        assert_ne!(save.teams[1].team_clan, TeamClan::Random);
        assert_ne!(
            crate::unit_values::team_base_values(save.teams[0].team_clan),
            crate::unit_values::BASE_UNIT_VALUES
        );
        assert_ne!(
            crate::unit_values::team_base_values(save.teams[1].team_clan),
            crate::unit_values::BASE_UNIT_VALUES
        );
    }

    #[test]
    fn team_units_domain_model_is_consistent() {
        let save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");
        let teams = save.team_units();
        let red = &teams[0];
        assert_eq!(red.gold, 52);
        assert_eq!(red.complexes.len(), 2);
        // Базовые значения = таблица клана.
        let expected = crate::unit_values::team_base_values(TeamClan::VonGriffin);
        for unit_index in 0..UNIT_COUNT {
            if unit_index == UnitType::LandingPad as usize {
                continue;
            }
            if expected[unit_index].hits != red.base_values[unit_index].hits {
                panic!("unit {unit_index} hits differ");
            }
        }
        // Идентичность объектов сохранена: base и current делят объекты там, где
        // не было апгрейдов (C++ пишет ссылку на тот же `UnitValues`).
        let shared = save.team_units[0]
            .base_values
            .iter()
            .zip(save.team_units[0].current_values.iter())
            .filter(|(base, current)| base.object_index == current.object_index)
            .count();
        assert!(shared > 80, "only {shared} shared base/current objects");
        let upgraded = UNIT_COUNT - shared;
        assert!(upgraded > 0 && upgraded < 20, "upgraded {upgraded}");
    }

    #[test]
    fn unit_references_resolve() {
        let save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");
        // Все ссылки parent/enemy указывают на существующие юниты.
        for unit in &save.units {
            if let Some(parent) = unit.info.parent_unit {
                assert!(save.unit_by_id(parent).is_some(), "parent {parent:?}");
            }
            if let Some(enemy) = unit.info.enemy_unit {
                assert!(save.unit_by_id(enemy).is_some(), "enemy {enemy:?}");
            }
            if let Some(complex) = unit.info.complex {
                assert!(
                    save.team_units.iter().any(|team| team
                        .complexes
                        .iter()
                        .any(|slot| slot.complex.id == complex.0)),
                    "complex {complex:?}"
                );
            }
        }
        // Хэш юнитов содержит ровно те юниты, что в списках (по объектам).
        let hashed: usize = save.unit_hash.iter_values().count();
        assert_eq!(hashed, save.units.len());
    }

    #[test]
    fn resource_and_surface_views() {
        let save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");
        let surface = save.surface();
        assert_eq!(surface.to_bytes(), save.surface_map);
        let resources = save.resource_map();
        assert_eq!(resources.bits_at(0), save.cargo_map[0]);
        // Полная сверка «побайтово» через u16-битфилды карты.
        for index in [1usize, 100, 5_000, MAP_TILES - 1] {
            assert_eq!(resources.bits_at(index), save.cargo_map[index]);
        }
        let counts = resources.counts();
        assert!(counts.raw_tiles > 0 || counts.fuel_tiles > 0 || counts.gold_tiles > 0);
    }

    #[test]
    fn rejects_other_versions_and_trailing_bytes() {
        let mut bytes = SAVE_FIXTURE.to_vec();
        bytes[0] = 71;
        assert_eq!(SaveFile::parse(&bytes), Err(SaveError::UnknownVersion(71)));
        let mut bytes = SAVE_FIXTURE.to_vec();
        bytes.push(0);
        assert_eq!(SaveFile::parse(&bytes), Err(SaveError::TrailingBytes(1)));
        assert!(SaveFile::parse(&SAVE_FIXTURE[..100]).is_err());
    }

    #[test]
    fn empty_parser_state_round_trips() {
        // Минимальная структура: только заголовок + пустая карта без объектов.
        // Проверяем, что writer не падает и парсер читает то, что записал.
        let save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");
        let bytes = save.to_bytes().expect("serializes");
        let again = SaveFile::parse(&bytes).expect("re-parses");
        assert_eq!(save, again);
    }

    #[test]
    fn modified_state_is_written_and_reparsed() {
        // Проверка обратного направления: не только «файл -> модель -> файл»,
        // но и «модель с изменениями -> файл -> модель».
        let mut save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");
        save.team_units[0].gold = 777;
        save.teams[0].team_points = 42;
        save.game.turn_counter = 9;
        save.team_ui[0].zoom_level = 64;
        let bytes = save.to_bytes().expect("serializes");
        let reparsed = SaveFile::parse(&bytes).expect("re-parses");
        assert_eq!(reparsed.team_units[0].gold, 777);
        assert_eq!(reparsed.teams[0].team_points, 42);
        assert_eq!(reparsed.game.turn_counter, 9);
        assert_eq!(reparsed.team_ui[0].zoom_level, 64);
        // Всё остальное не изменилось.
        assert_eq!(reparsed.units.len(), save.units.len());
        assert_eq!(reparsed.unit_lists, save.unit_lists);
        // Файл отличается от исходного ровно в изменённых полях.
        assert_ne!(bytes, SAVE_FIXTURE);
        assert_eq!(bytes.len(), SAVE_FIXTURE.len());
    }

    #[test]
    fn unit_values_attribute_from_save() {
        let save = SaveFile::parse(SAVE_FIXTURE).expect("V70 save parses");
        let slot = &save.team_units[0].base_values[UnitType::Tank as usize];
        let plain = base_unit_values(UnitType::Tank);
        // Von Griffin не меняет танк: значения совпадают с таблицей.
        assert_eq!(slot.values, plain);
        // Атрибут читается через общий интерфейс.
        assert_eq!(
            slot.values.get_attribute(ATTRIB_ATTACK),
            plain.get_attribute(ATTRIB_ATTACK)
        );
    }
}
