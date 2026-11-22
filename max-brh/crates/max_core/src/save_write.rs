//! Запись V70 из живого состояния — `GameState -> SaveFile` (P3).
//!
//! Оракул: `max-cpp-port/src/saveload.cpp` (`SaveLoad_Save`), `teamunits.cpp`
//! (`TeamUnits::FileSave`), `unitinfo.cpp` (`UnitInfo::FileSave`),
//! `aiplayer.cpp` (`AiPlayer::FileSave`), `hash.cpp`/`heatmap.cpp`
//! (`FileSave`), плюс парсер [`crate::save`] — он читает ровно эти же поля.
//!
//! # Как строится объектный номер
//!
//! V70 — граф объектов с номерами по порядку **первого появления** при обходе
//! (см. [`crate::save`]). Номера в `GameState` не хранятся, поэтому
//! [`build_save_file`] присваивает их заново, повторяя порядок обхода
//! C++-писателя:
//!
//! 1. `TeamUnits` команд 0..3: 93 базовых `UnitValues`, 93 текущих (если
//!    текущие совпадают с базовыми — объект общий) и комплексы;
//! 2. пять списков `UnitInfo` в порядке файла; тело юнита — путь,
//!    `base_values`, комплекс, `parent_unit`, `enemy_unit` (вложенные юниты
//!    впервые появляются здесь);
//! 3. остальные юниты (вне списков) — в конце, чтобы ничего не потерять.
//!
//! Для эталонного файла, загруженного [`GameState::from_save`], такой обход
//! повторяет исходные номера **побайтово** — это проверяет
//! `tests/save_write.rs::save1_load_to_save_is_byte_identical`.
//!
//! # Что не восстанавливается из рантайма (честные дефолты)
//!
//! * `markers[10]`, `screen_locations[6]`, `selected_unit`, `zoom_level`,
//!   `camera_position`, `display_button_*` — UI-поля `TeamInfo` (state-model
//!   §6.3); C++ держит их в `CTInfo`, наш `GameState` — в
//!   [`SaveMeta::team_ui`] (переносятся из загруженного сейва, при создании
//!   партии — дефолты `CTInfo::Reset`).
//! * `AiPlayer::unused_v70`/`target_team_v70` — V70-поля, которые C++
//!   **читает и выбрасывает** (`aiplayer.cpp`); значения переносятся через
//!   [`SaveMeta`], чтобы round-trip был побайтовым.
//! * `stats_gold_spent_on_upgrades` — C++ клампит ≥ 0 при загрузке, сырое
//!   `int16_t` лежит в [`SaveMeta::team_gold_spent_raw`].
//! * Тепловые карты: C++ `HeatMap::LoadV70` клампит отрицательные в 0, а в
//!   памяти клетка — `uint32_t`; обратно пишется `min(127, value)` как `i8`.
//! * `MessageLog` — журнал UI; переносится из [`SaveMeta::message_logs`].
//! * `movement_wait_count` (не входит в V70), `experience` (у
//!   `COMMANDO`/`REGENERATING_UNIT` пакуется в `storage`), `transfer_cargo` и
//!   `stealth_dice_roll` (V70 хранит их в `move_to_grid`) — не пишутся, как и
//!   в C++.

use std::collections::BTreeMap;

use crate::enums::{FileType, Opponent, Planet, TeamClan};
use crate::game::GameState;
use crate::geo::{Point, MAP_TILES};
use crate::hash::{MapHash, MapHashObject, UnitHash};
use crate::save::{
    AiPlayerState, GameManagerState, HeatMap, IniOptions, IniPreferences, MessageLogEntry, SaveFile,
    SaveFormatHeader, SaveTeamUnits, SaveUnit, SavedComplex, SavedUnitValues, SpottedUnit,
    SAVE_TEAM_COUNT,
};
use crate::team::{Complex, TeamInfoUi, TeamKind};
use crate::unit::{UnitId, UnitInfo, UnitListKind};
use crate::unit_values::{UnitValues, UNIT_COUNT};

/// Размер `Hash_UnitHash` в V70-файле (`hash.cpp`: 512 вёдер у SAVE1/SAVE2).
pub const SAVE_UNIT_HASH_SIZE: u16 = 512;

/// Данные V70, которые симуляция не использует, но которые нужны для записи
/// сейва 1:1 (P3).
///
/// Живёт в [`GameState::save_meta`]. При загрузке заполняется из файла
/// ([`SaveMeta::from_save`]), при создании партии — из настроек
/// ([`SaveMeta::default`] + поля `max_app`). В канонический дайджест состояния
/// (`GameState::snapshot_bytes`) **не входит**: это не игровое состояние.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveMeta {
    /// `save_game_type` заголовка (`FileType`).
    pub save_game_type: FileType,
    /// `save_name[30]` — имя сохранения сырыми байтами.
    pub save_name: [u8; 30],
    /// `header.world` — планета партии.
    pub world: Planet,
    /// `team_name[4][30]` — имена команд.
    pub team_names: [[u8; 30]; 4],
    /// `rng_seed` — зерно ГПСЧ партии (V70 не хранит состояние `Randomizer`).
    pub rng_seed: u32,
    /// `header.opponent` — сложность ИИ.
    pub opponent: Opponent,
    /// `turn_timer_time` — таймер хода, сек.
    pub turn_timer_time: u16,
    /// `endturn_time` — таймер конца хода, сек.
    pub endturn_time: u16,
    /// `IniOptions` (12 × `signed int`).
    pub ini_options: IniOptions,
    /// `IniPreferences` (7 × `signed int`).
    pub ini_preferences: IniPreferences,
    /// UI-поля `TeamInfo` × 4 (`markers`/`screen_locations`/камера/зум/кнопки).
    pub team_ui: [TeamInfoUi; 4],
    /// Сырые `stats_gold_spent_on_upgrades` × 4 (V70 `int16_t`).
    pub team_gold_spent_raw: [i16; 4],
    /// `team_type[PLAYER_TEAM_ALIEN]` заголовка (V70 пишет 5-й слот, `CTInfo`
    /// его не хранит).
    pub alien_team_type: TeamKind,
    /// `team_clan[PLAYER_TEAM_ALIEN]` заголовка.
    pub alien_team_clan: TeamClan,
    /// `AiPlayer::unused_v70` × 4 — C++ читает и выбрасывает.
    pub ai_unused_v70: [i16; 4],
    /// `AiPlayer::target_team` × 4 (V70) — C++ читает и выбрасывает.
    pub ai_target_team_v70: [i16; 4],
    /// Был ли в файле блок `info_map` у ИИ команды (`ReadObjectCount != 0`).
    ///
    /// У эталонных V70-сейвов карт ИИ нет (C++ пишет `item_count = 0`), а
    /// рантайм `max_core` всегда держит карту 112×112 — без этого флага запись
    /// добавила бы 12 544 байта на каждую компьютерную команду.
    pub ai_has_info_map: [bool; 4],
    /// Был ли в файле блок `mine_map`.
    pub ai_has_mine_map: [bool; 4],
    /// Порядок `UnitHash` как в файле: вёдра → `UnitId`.
    ///
    /// C++ вставляет юнитов в ведро при создании (`UnitsManager_DeployUnit`),
    /// поэтому порядок внутри ведра — порядок создания, который в рантайме не
    /// выводится ни из id, ни из списков. Переносим его из загруженного сейва,
    /// а новые юниты дописываем в конец ведра.
    pub unit_hash_order: Vec<Vec<UnitId>>,
    /// Журналы сообщений × 4 (`MessageLogList`).
    pub message_logs: [Vec<MessageLogEntry>; 4],
    /// Остаток таймера хода в `GameManagerState` (сек).
    pub turn_timer: u16,
    /// Размер `Hash_UnitHash` в файле (C++ 512; в рантайме `max_core` — 8192).
    pub unit_hash_size: u16,
    /// Размер `Hash_MapHash` в файле (V70-сейвы оригинала — 512).
    pub map_hash_size: u16,
    /// `x_shift` из заголовка `MapHash` (у 512 вёдер — 2).
    pub map_hash_x_shift: u16,
    /// Клетки `MapHash` в порядке файла: `(x, y, юниты)`.
    ///
    /// C++ при загрузке **перехэширует** таблицу в размер текущей карты
    /// (`hash.cpp:240`), поэтому из рантайма исходный порядок вёдер не
    /// выводится; для побайтовой записи он переносится здесь. При сохранении
    /// порядок сохраняется для неизменившихся клеток, новые дописываются в
    /// конец ([`build_save_file`]).
    pub map_hash_cells: Vec<(u16, u16, Vec<UnitId>)>,
}

impl Default for SaveMeta {
    fn default() -> Self {
        Self {
            save_game_type: FileType::Custom,
            save_name: [0; 30],
            world: Planet::Snowcrab,
            team_names: [[0; 30]; 4],
            rng_seed: 0,
            opponent: Opponent::Average,
            turn_timer_time: 0,
            endturn_time: 0,
            ini_options: IniOptions::default(),
            ini_preferences: IniPreferences::default(),
            team_ui: std::array::from_fn(|_| TeamInfoUi::default()),
            team_gold_spent_raw: [0; 4],
            alien_team_type: TeamKind::None,
            alien_team_clan: TeamClan::Random,
            ai_unused_v70: [0; 4],
            ai_target_team_v70: [0; 4],
            ai_has_info_map: [false; 4],
            ai_has_mine_map: [false; 4],
            unit_hash_order: Vec::new(),
            message_logs: std::array::from_fn(|_| Vec::new()),
            turn_timer: 0,
            unit_hash_size: SAVE_UNIT_HASH_SIZE,
            map_hash_size: SAVE_UNIT_HASH_SIZE,
            map_hash_x_shift: 2,
            map_hash_cells: Vec::new(),
        }
    }
}

impl SaveMeta {
    /// Снимает метаданные с разобранного V70-файла.
    pub fn from_save(save: &SaveFile) -> Self {
        let mut meta = Self::default();
        meta.save_game_type = save.header.save_game_type;
        meta.save_name = save.header.save_name;
        meta.world = save.header.world;
        meta.team_names = save.header.team_names;
        meta.rng_seed = save.header.rng_seed;
        meta.opponent = save.header.opponent;
        meta.turn_timer_time = save.header.turn_timer_time;
        meta.endturn_time = save.header.endturn_time;
        meta.ini_options = save.ini_options;
        meta.ini_preferences = save.ini_preferences;
        meta.team_ui = save.team_ui.clone();
        meta.team_gold_spent_raw = save.team_gold_spent_raw;
        meta.alien_team_type = save.header.team_type[SAVE_TEAM_COUNT];
        meta.alien_team_clan = save.header.team_clan[SAVE_TEAM_COUNT];
        for team in 0..SAVE_TEAM_COUNT {
            if let Some(ai) = save.ai_players[team].as_deref() {
                meta.ai_unused_v70[team] = ai.unused_v70;
                meta.ai_target_team_v70[team] = ai.target_team_v70;
                meta.ai_has_info_map[team] = ai.info_map.is_some();
                meta.ai_has_mine_map[team] = ai.mine_map.is_some();
            }
            meta.message_logs[team] = save.message_logs[team].clone();
        }
        // `UnitHash` файла ссылается на объекты → переводим в `UnitId`.
        let ids: BTreeMap<u16, UnitId> = save
            .units
            .iter()
            .map(|unit| (unit.object_index, unit.info.id))
            .collect();
        meta.unit_hash_order = save
            .unit_hash
            .buckets()
            .iter()
            .map(|bucket| {
                bucket
                    .iter()
                    .filter_map(|object_index| ids.get(object_index).copied())
                    .collect()
            })
            .collect();
        meta.turn_timer = save.game.turn_timer;
        meta.unit_hash_size = save.unit_hash.size();
        meta.map_hash_size = save.map_hash.size();
        meta.map_hash_x_shift = save.map_hash.x_shift();
        meta.map_hash_cells = save
            .map_hash
            .iter_objects()
            .map(|object| {
                (
                    object.x,
                    object.y,
                    object
                        .units
                        .iter()
                        .filter_map(|object_index| ids.get(object_index).copied())
                        .collect(),
                )
            })
            .collect();
        meta
    }

    /// Имя сохранения строкой.
    pub fn save_name(&self) -> String {
        fixed_name(&self.save_name)
    }

    /// Имя команды строкой.
    pub fn team_name(&self, team: usize) -> String {
        self.team_names
            .get(team)
            .map(|name| fixed_name(name))
            .unwrap_or_default()
    }

    /// Записывает строку в 30-байтовое поле имени (NUL-терминатор, обрезка).
    pub fn set_name(field: &mut [u8; 30], value: &str) {
        field.fill(0);
        for (slot, byte) in field.iter_mut().zip(value.as_bytes()) {
            *slot = *byte;
        }
    }
}

fn fixed_name(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

// ---------------------------------------------------------------------------
// Объектная раскладка
// ---------------------------------------------------------------------------

/// Ключ объекта V70: чем именно является объект в графе.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ObjectKey {
    /// `UnitValues` команды: базовые или текущие значения типа.
    TeamValues {
        /// Команда.
        team: u8,
        /// `false` — текущие значения, `true` — базовые.
        base: bool,
        /// Индекс типа юнита 0..92.
        unit_type: u16,
    },
    /// `Complex` из списка `TeamUnits::complexes`.
    TeamComplex {
        /// Команда.
        team: u8,
        /// `Complex::id`.
        id: i16,
    },
    /// `UnitValues` конкретного юнита (`UnitInfo::base_values`).
    UnitValues {
        /// `UnitId` юнита.
        id: u16,
    },
    /// Путь юнита (`AirPath`/`BuilderPath`/`GroundPath`).
    UnitPath {
        /// `UnitId` юнита.
        id: u16,
    },
    /// `Complex`, на который ссылается юнит, но которого нет в списке команды
    /// (битый сейв/рассинхронизация): пишется минимальное тело.
    OrphanComplex {
        /// Команда.
        team: u8,
        /// `Complex::id`.
        id: i16,
    },
    /// `UnitInfo`.
    Unit {
        /// `UnitId` юнита.
        id: u16,
    },
}

/// Присваивание номеров объектов в порядке обхода (C++ `SmartFileWriter`).
#[derive(Debug, Default)]
struct Layout {
    next: u16,
    objects: BTreeMap<ObjectKey, u16>,
}

impl Layout {
    fn new() -> Self {
        Self {
            next: 1,
            objects: BTreeMap::new(),
        }
    }

    /// Номер объекта для ключа; новый объект получает следующий номер.
    fn object(&mut self, key: ObjectKey) -> u16 {
        if let Some(index) = self.objects.get(&key) {
            return *index;
        }

        let index = self.next;
        if self.next < u16::MAX {
            self.next += 1;
        }
        self.objects.insert(key, index);
        index
    }

    /// Номер уже назначенного объекта.
    fn get(&self, key: ObjectKey) -> Option<u16> {
        self.objects.get(&key).copied()
    }

    /// Номер объекта `UnitInfo` (0 — юнита нет).
    fn unit(&self, id: Option<UnitId>) -> u16 {
        id.and_then(|id| self.get(ObjectKey::Unit { id: id.0 }))
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Сборка SaveFile
// ---------------------------------------------------------------------------

/// Собирает V70-представление живого состояния.
///
/// Все объекты нумеруются заново в порядке обхода C++-писателя (см. модульную
/// документацию), поэтому загруженный и сразу сохранённый эталонный сейв
/// получается побайтово тем же (см. `tests/save_write.rs`).
pub fn build_save_file(state: &GameState) -> SaveFile {
    let meta = &state.save_meta;
    let mut layout = Layout::new();

    // --- 1. TeamUnits × 4: значения и комплексы ---
    let mut team_units: [SaveTeamUnits; SAVE_TEAM_COUNT] = std::array::from_fn(|_| SaveTeamUnits {
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

    for team in 0..SAVE_TEAM_COUNT {
        let units = &state.team_units[team];
        let mut base_values: [SavedUnitValues; UNIT_COUNT] = std::array::from_fn(|_| {
            SavedUnitValues {
                object_index: 0,
                values: UnitValues::new(),
            }
        });
        let mut current_values: [SavedUnitValues; UNIT_COUNT] = std::array::from_fn(|_| {
            SavedUnitValues {
                object_index: 0,
                values: UnitValues::new(),
            }
        });

        // C++ `TeamUnits::FileSave` пишет **сначала все базовые** значения, затем
        // все текущие — объектные номера присваиваются в этом же порядке.
        let mut base_index: [u16; UNIT_COUNT] = [0; UNIT_COUNT];
        for unit_type in 0..UNIT_COUNT {
            base_index[unit_type] = layout.object(ObjectKey::TeamValues {
                team: team as u8,
                base: true,
                unit_type: unit_type as u16,
            });
            base_values[unit_type] = SavedUnitValues {
                object_index: base_index[unit_type],
                values: units.base_values[unit_type],
            };
        }

        for unit_type in 0..UNIT_COUNT {
            // C++ `TeamUnits` держит `UnitValues*`: пока исследований не было,
            // current и base — один объект. В рантайме остаются только значения,
            // поэтому общность восстанавливается по равенству: апгрейд всегда
            // меняет хотя бы одну характеристику (`upgrade_offset_factor`).
            let current_index = if units.current_values[unit_type] == units.base_values[unit_type] {
                base_index[unit_type]
            } else {
                layout.object(ObjectKey::TeamValues {
                    team: team as u8,
                    base: false,
                    unit_type: unit_type as u16,
                })
            };
            current_values[unit_type] = SavedUnitValues {
                object_index: current_index,
                values: units.current_values[unit_type],
            };
        }

        let mut complexes = Vec::with_capacity(units.complexes.len());
        for complex in &units.complexes {
            let index = layout.object(ObjectKey::TeamComplex {
                team: team as u8,
                id: complex.id,
            });
            complexes.push(SavedComplex {
                object_index: index,
                complex: *complex,
            });
        }

        team_units[team] = SaveTeamUnits {
            // V70 пишет `signed short`; C++ хранит `uint32_t`.
            gold: units.gold as u16,
            base_values,
            current_values,
            complexes,
        };
    }

    // --- 2-3. Юниты: списки и тела ---
    let mut units: Vec<SaveUnit> = Vec::with_capacity(state.unit_count());
    let mut unit_lists: [Vec<u16>; 5] = Default::default();

    for kind in UnitListKind::ALL {
        let mut list = Vec::with_capacity(state.unit_list(kind).len());
        for &id in state.unit_list(kind) {
            if let Some(object_index) = walk_unit(state, id, &mut layout, &mut units, 0) {
                list.push(object_index);
            }
        }
        unit_lists[kind.index()] = list;
    }

    // Юниты, которых нет ни в одном списке (например, только что созданный и
    // ещё не добавленный в список): пишем ссылкой 0 в списки не попадают, но
    // объект обязан быть в файле, иначе он потеряется.
    for id in state.unit_ids_in_order() {
        walk_unit(state, id, &mut layout, &mut units, 0);
    }

    units.sort_by_key(|unit| unit.object_index);

    // --- UnitHash: порядок вёдер как в загруженном файле + новые юниты ---
    //
    // C++ вставляет юнита в ведро при создании (`UnitsManager_DeployUnit`),
    // поэтому порядок внутри ведра — порядок создания: он не выводится из
    // id/списков и переносится через [`SaveMeta::unit_hash_order`].
    let mut unit_hash: UnitHash<u16> = UnitHash::new(meta.unit_hash_size.max(1));
    let mut hashed: BTreeMap<UnitId, ()> = BTreeMap::new();
    for bucket in &meta.unit_hash_order {
        for &id in bucket {
            let object_index = layout.unit(Some(id));
            if object_index == 0 || hashed.contains_key(&id) {
                continue;
            }
            hashed.insert(id, ());
            unit_hash.push_at(u32::from(id.0), object_index);
        }
    }
    for unit in &units {
        if hashed.contains_key(&unit.info.id) {
            continue;
        }
        hashed.insert(unit.info.id, ());
        unit_hash.push_at(u32::from(unit.info.id.0), unit.object_index);
    }

    // --- MapHash: порядок клеток как в файле + новые клетки ---
    //
    // Размер и x_shift берутся из [`SaveMeta`]: C++ при загрузке перехэширует
    // таблицу в размер карты, поэтому 512 вёдер эталона из рантайма не
    // восстановить. Клетки, которые были в файле, пишутся в прежнем порядке с
    // **текущим** составом юнитов; появившиеся в игре клетки — в конец.
    let mut map_hash: MapHash<u16> =
        MapHash::new(meta.map_hash_size.max(crate::hash::HASH_MAP_HASH_SIZE_MIN));
    let mut current_cells: BTreeMap<(u16, u16), Vec<u16>> = BTreeMap::new();
    for object in state.map_hash.iter_objects() {
        let mut mapped = Vec::with_capacity(object.units.len());
        for &id in &object.units {
            let object_index = layout.unit(Some(id));
            if object_index != 0 {
                mapped.push(object_index);
            }
        }
        if mapped.is_empty() {
            continue;
        }
        current_cells.insert((object.x, object.y), mapped);
    }
    for (x, y, _) in &meta.map_hash_cells {
        let Some(mapped) = current_cells.remove(&(*x, *y)) else {
            // Клетка опустела — объекта в файле нет (C++ удаляет пустые).
            continue;
        };
        map_hash.bucket_mut(*x, *y).push(MapHashObject {
            x: *x,
            y: *y,
            units: mapped,
        });
    }
    for ((x, y), units) in current_cells {
        map_hash.bucket_mut(x, y).push(MapHashObject { x, y, units });
    }

    // --- тепловые карты × 4 (только у играющих команд) ---
    let mut heat_maps: [Option<Box<HeatMap>>; SAVE_TEAM_COUNT] = Default::default();
    for team in 0..SAVE_TEAM_COUNT {
        if state.teams[team].team_type == TeamKind::None {
            continue;
        }
        let cells = state.ai.heat_maps[team].cells();
        let mut map = Box::new(HeatMap {
            complete: [0; MAP_TILES],
            stealth_sea: [0; MAP_TILES],
            stealth_land: [0; MAP_TILES],
        });
        for (index, cell) in cells.iter().take(MAP_TILES).enumerate() {
            map.complete[index] = heat_value(cell.complete);
            map.stealth_sea[index] = heat_value(cell.stealth_sea);
            map.stealth_land[index] = heat_value(cell.stealth_land);
        }
        heat_maps[team] = Some(map);
    }

    // --- журналы сообщений × 4 ---
    let mut message_logs: [Vec<MessageLogEntry>; SAVE_TEAM_COUNT] =
        std::array::from_fn(|_| Vec::new());
    for (team, logs) in message_logs.iter_mut().enumerate() {
        for entry in &meta.message_logs[team] {
            let mut entry = entry.clone();
            entry.unit_object_index = layout.unit(entry.unit);
            logs.push(entry);
        }
    }

    // --- AiPlayer × 4 (только компьютерные команды) ---
    let mut ai_players: [Option<Box<AiPlayerState>>; SAVE_TEAM_COUNT] = Default::default();
    for team in 0..SAVE_TEAM_COUNT {
        if state.teams[team].team_type != TeamKind::Computer {
            continue;
        }
        let player = &state.ai.players[team];
        let mut spotted_units = Vec::with_capacity(player.spotted_units.len());
        for spotted in &player.spotted_units {
            let Some(unit) = spotted.unit else { continue };
            let object_index = layout.unit(Some(unit));
            if object_index == 0 {
                continue;
            }
            spotted_units.push(SpottedUnit {
                unit_object_index: object_index,
                unit: Some(unit),
                team: u16::from(spotted.team.0),
                visible_to_team: spotted.visible_to_team,
                last_position: spotted.last_position,
            });
        }
        ai_players[team] = Some(Box::new(AiPlayerState {
            player_team: team as u16,
            strategy: player.strategy as u8,
            unused_v70: meta.ai_unused_v70[team],
            greenhouse_ratio: player.greenhouse_ratio,
            minefield_density: player.minefield_density,
            target_team_v70: meta.ai_target_team_v70[team],
            spotted_units,
            info_map: if meta.ai_has_info_map[team] {
                map_bytes(&player.info_map)
            } else {
                None
            },
            mine_map: if meta.ai_has_mine_map[team] {
                mine_map_bytes(&player.mine_map)
            } else {
                None
            },
            target_location: player.target_location,
        }));
    }

    // --- заголовок и настройки ---
    let mut team_type = [TeamKind::None; SAVE_TEAM_COUNT + 1];
    let mut team_clan = [TeamClan::Random; SAVE_TEAM_COUNT + 1];
    for team in 0..SAVE_TEAM_COUNT {
        team_type[team] = state.teams[team].team_type;
        team_clan[team] = state.teams[team].team_clan;
    }
    team_type[SAVE_TEAM_COUNT] = meta.alien_team_type;
    team_clan[SAVE_TEAM_COUNT] = meta.alien_team_clan;

    let header = SaveFormatHeader {
        version: crate::save::SAVE_VERSION_V70,
        save_game_type: meta.save_game_type,
        save_name: meta.save_name,
        world: meta.world,
        mission_index: state.mission_index,
        team_names: meta.team_names,
        team_type,
        team_clan,
        rng_seed: meta.rng_seed,
        opponent: meta.opponent,
        turn_timer_time: meta.turn_timer_time,
        endturn_time: meta.endturn_time,
        play_mode: state.play_mode,
    };

    let game = GameManagerState {
        active_turn_team: state.active_turn_team,
        player_team: state.player_team,
        turn_counter: state.turn_counter,
        game_state: state.game_state,
        turn_timer: meta.turn_timer,
    };

    SaveFile {
        header,
        ini_options: meta.ini_options,
        ini_preferences: meta.ini_preferences,
        surface_map: state.map.surface.to_bytes(),
        cargo_map: {
            let mut cargo = [0u16; MAP_TILES];
            for (slot, entry) in cargo.iter_mut().zip(state.map.resources.tiles().iter()) {
                *slot = entry.bits();
            }
            cargo
        },
        teams: std::array::from_fn(|team| state.teams[team].clone()),
        team_ui: meta.team_ui.clone(),
        team_gold_spent_raw: meta.team_gold_spent_raw,
        game,
        team_units,
        unit_lists,
        units,
        unit_hash,
        map_hash,
        heat_maps,
        message_logs,
        ai_players,
    }
}

/// Клетка тепловой карты → `int8_t` файла (C++ `HeatMap::FileSave` пишет
/// `legacy`-значение; в памяти клетка — `uint32_t`).
fn heat_value(value: u32) -> i8 {
    value.min(i8::MAX as u32) as i8
}

/// `info_map` (112×112) → массив файла; короткая карта дополняется
/// `INFO_MAP_NO_INFO` (0).
fn map_bytes(values: &[u8]) -> Option<Box<[u8; MAP_TILES]>> {
    let mut map = Box::new([0u8; MAP_TILES]);
    for (slot, value) in map.iter_mut().zip(values.iter()) {
        *slot = *value;
    }
    Some(map)
}

/// `mine_map` (112×112) → массив файла.
fn mine_map_bytes(values: &[i8]) -> Option<Box<[i8; MAP_TILES]>> {
    let mut map = Box::new([0i8; MAP_TILES]);
    for (slot, value) in map.iter_mut().zip(values.iter()) {
        *slot = *value;
    }
    Some(map)
}

/// Номер объекта `UnitValues` для `base_values` юнита.
///
/// В C++ `UnitInfo::base_values` — указатель на `UnitValues` команды (базовые
/// или текущие — смотря на момент создания юнита), поэтому объект обычно
/// **общий** с `TeamUnits`. В рантайме остаются только значения, поэтому
/// правило восстановления: значения юнита совпали с базовыми команды → базовый
/// объект, с текущими → текущий, иначе (апгрейд, которого нет в таблице) —
/// собственный объект юнита. Для SAVE1/SAVE2 правило совпадает с файлом
/// (проверяет `tests/save_write.rs`).
fn unit_values_object(state: &GameState, info: &UnitInfo, layout: &mut Layout) -> u16 {
    let team = info.team.0 as usize;
    let unit_type = info.unit_type as usize;
    if let Some(units) = state.team_units.get(team) {
        if unit_type < UNIT_COUNT {
            if info.base_values == units.base_values[unit_type] {
                return layout.object(ObjectKey::TeamValues {
                    team: info.team.0,
                    base: true,
                    unit_type: info.unit_type as u16,
                });
            }
            if info.base_values == units.current_values[unit_type] {
                return layout.object(ObjectKey::TeamValues {
                    team: info.team.0,
                    base: false,
                    unit_type: info.unit_type as u16,
                });
            }
        }
    }

    layout.object(ObjectKey::UnitValues { id: info.id.0 })
}

/// Пишет тело юнита и его вложенные объекты; возвращает номер объекта.
///
/// Порядок вложенных объектов — как в C++ `UnitInfo::FileSave`: путь,
/// `base_values`, комплекс, `parent_unit`, `enemy_unit`.
fn walk_unit(
    state: &GameState,
    id: UnitId,
    layout: &mut Layout,
    units: &mut Vec<SaveUnit>,
    depth: usize,
) -> Option<u16> {
    if depth > crate::save::MAX_UNIT_DEPTH {
        return None;
    }
    if let Some(object_index) = layout.get(ObjectKey::Unit { id: id.0 }) {
        return Some(object_index);
    }

    let info: &UnitInfo = state.unit(id)?;
    // Презентация хранится рядом с юнитом в `GameState` (Ф5.3/B) и пишется в
    // V70 в исходном порядке байтов (`save::write_unit_body`).
    let presentation = state.presentation(id).cloned().unwrap_or_default();
    let object_index = layout.object(ObjectKey::Unit { id: id.0 });
    let team = info.team.0;

    let path_object_index = if info.path.is_some() {
        layout.object(ObjectKey::UnitPath { id: id.0 })
    } else {
        0
    };

    let base_values_object_index = unit_values_object(state, info, layout);

    let (complex_object_index, complex_value) = match info.complex {
        Some(complex_id) => {
            let in_team = state
                .team_units
                .get(team as usize)
                .and_then(|units| units.get_complex(complex_id))
                .copied();
            match in_team {
                Some(complex) => (
                    layout.object(ObjectKey::TeamComplex {
                        team,
                        id: complex_id.0,
                    }),
                    complex,
                ),
                None => {
                    // Комплекс потерялся (юнит ссылается на несуществующий id):
                    // пишем минимальное тело, чтобы ссылка осталась валидной.
                    let value = Complex::new(complex_id.0);
                    (
                        layout.object(ObjectKey::OrphanComplex {
                            team,
                            id: complex_id.0,
                        }),
                        value,
                    )
                }
            }
        }
        None => (0, Complex::new(0)),
    };

    let parent_object_index = match info.parent_unit {
        Some(parent) if parent != id => walk_unit(state, parent, layout, units, depth + 1).unwrap_or(0),
        _ => 0,
    };
    let enemy_object_index = match info.enemy_unit {
        Some(enemy) if enemy != id => walk_unit(state, enemy, layout, units, depth + 1).unwrap_or(0),
        _ => 0,
    };

    units.push(SaveUnit {
        object_index,
        info: info.clone(),
        presentation,
        path_object_index,
        base_values_object_index,
        complex_object_index,
        complex_value,
        parent_object_index,
        enemy_object_index,
    });

    Some(object_index)
}

impl GameState {
    /// V70-представление состояния (P3): объектная нумерация пересобирается
    /// в порядке C++-писателя, см. [`build_save_file`].
    pub fn to_save(&self) -> SaveFile {
        build_save_file(self)
    }

    /// Байты V70-сейва текущей партии.
    pub fn to_save_bytes(&self) -> Result<Vec<u8>, crate::save::SaveError> {
        self.to_save().to_bytes()
    }

    /// Настройки/служебные поля сейва (V70).
    pub fn save_meta(&self) -> &SaveMeta {
        &self.save_meta
    }

    /// Заменяет настройки/служебные поля сейва (используется при создании
    /// партии и при сохранении из UI).
    pub fn set_save_meta(&mut self, meta: SaveMeta) {
        self.save_meta = meta;
    }

    /// Дописывает сообщение в журнал команды (`MessageLogList`): попадает в
    /// V70-сейв, в дайджест состояния не входит.
    pub fn push_message(&mut self, team: u8, text: &str, point: Point, unit: Option<UnitId>) {
        let index = usize::from(team).min(SAVE_TEAM_COUNT - 1);
        let mut bytes = text.as_bytes().to_vec();
        // V70 пишет `strlen + 1` — текст с завершающим NUL.
        bytes.push(0);
        self.save_meta.message_logs[index].push(MessageLogEntry {
            text: bytes,
            unit_object_index: 0,
            unit,
            point,
            is_alert_message: false,
            id: 0xFFFF,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_state_builds_a_parsable_save() {
        let state = GameState::new();
        let save = state.to_save();
        let bytes = save.to_bytes().expect("writes");
        let reparsed = SaveFile::parse(&bytes).expect("parses");
        assert_eq!(reparsed.header.version, crate::save::SAVE_VERSION_V70);
        assert_eq!(reparsed.units.len(), 0);
        assert_eq!(reparsed.game.turn_counter, 0);
        assert_eq!(reparsed.team_units[0].base_values.len(), UNIT_COUNT);
        // Круг «модель -> файл -> модель» без юнитов стабилен.
        assert_eq!(reparsed.to_bytes().expect("writes"), bytes);
    }

    #[test]
    fn layout_numbers_objects_in_first_appearance_order() {
        let mut layout = Layout::new();
        let first = layout.object(ObjectKey::TeamValues {
            team: 0,
            base: true,
            unit_type: 0,
        });
        let second = layout.object(ObjectKey::TeamComplex { team: 0, id: 1 });
        assert_eq!((first, second), (1, 2));
        assert_eq!(
            layout.object(ObjectKey::TeamValues {
                team: 0,
                base: true,
                unit_type: 0
            }),
            1
        );
        assert_eq!(layout.object(ObjectKey::Unit { id: 7 }), 3);
        assert_eq!(layout.unit(Some(UnitId(7))), 3);
        assert_eq!(layout.unit(Some(UnitId(8))), 0);
        assert_eq!(layout.unit(None), 0);
    }

    #[test]
    fn heat_values_are_clamped_to_int8() {
        assert_eq!(heat_value(0), 0);
        assert_eq!(heat_value(12), 12);
        assert_eq!(heat_value(127), 127);
        assert_eq!(heat_value(1000), 127);
    }

    /// Порядок объектов писателя: номера идут подряд в порядке обхода.
    ///
    /// Ловит регрессию «базовые и текущие значения перемешаны» (C++ пишет
    /// сначала все базовые `UnitValues`, потом все текущие), из-за которой
    /// `SaveFile::to_bytes` падал с `ForwardReference`.
    #[test]
    fn object_numbers_follow_writer_order() {
        let save = crate::save::SaveFile::parse(include_bytes!("../tests/data/save1_v70.cam"))
            .expect("эталон разбирается");
        let built = GameState::from_save(&save).to_save();

        let mut next = 1u16;
        for (team, units) in built.team_units.iter().enumerate() {
            for (index, slot) in units.base_values.iter().enumerate() {
                assert_eq!(
                    slot.object_index, next,
                    "team {team}: базовое значение {index} должно идти подряд"
                );
                next += 1;
            }
            for (index, slot) in units.current_values.iter().enumerate() {
                if slot.object_index >= next {
                    assert_eq!(
                        slot.object_index, next,
                        "team {team}: текущее значение {index} должно идти подряд"
                    );
                    next += 1;
                }
            }
            for (index, slot) in units.complexes.iter().enumerate() {
                assert_eq!(
                    slot.object_index, next,
                    "team {team}: комплекс {index} должен идти подряд"
                );
                next += 1;
            }
        }
        // Номера юнитов уникальны, лежат после объектов команд и вместе со
        // всеми вложенными объектами образуют непрерывный диапазон 1..=N
        // (этого требует `SmartFileWriter`).
        let mut indices: Vec<u16> = Vec::new();
        for units in &built.team_units {
            for slot in units.base_values.iter().chain(units.current_values.iter()) {
                indices.push(slot.object_index);
            }
            for slot in &units.complexes {
                indices.push(slot.object_index);
            }
        }
        for unit in &built.units {
            indices.extend([
                unit.object_index,
                unit.path_object_index,
                unit.base_values_object_index,
                unit.complex_object_index,
                unit.parent_object_index,
                unit.enemy_object_index,
            ]);
        }
        indices.retain(|index| *index != 0);
        indices.sort_unstable();
        indices.dedup();
        assert_eq!(
            indices,
            (1..=indices.len() as u16).collect::<Vec<_>>(),
            "объектные номера должны быть непрерывны"
        );
    }
}
