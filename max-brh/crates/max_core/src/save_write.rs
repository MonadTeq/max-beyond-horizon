//! Writing V70 from live state — `GameState -> SaveFile` (P3).
//!
//! Oracle: `max-cpp-port/src/saveload.cpp` (`SaveLoad_Save`), `teamunits.cpp`
//! (`TeamUnits::FileSave`), `unitinfo.cpp` (`UnitInfo::FileSave`),
//! `aiplayer.cpp` (`AiPlayer::FileSave`), `hash.cpp`/`heatmap.cpp`
//! (`FileSave`), plus the [`crate::save`] parser — it reads exactly these fields.
//!
//! # How the object number is built
//!
//! V70 is an object graph numbered by order of **first appearance** during traversal
//! (see [`crate::save`]). The numbers are not stored in `GameState`, so
//! [`build_save_file`] assigns them anew, repeating the traversal order of
//! the C++ writer:
//!
//! 1. `TeamUnits` of teams 0..3: 93 base `UnitValues`, 93 current (if
//!    current equals base — the object is shared) and complexes;
//! 2. the five `UnitInfo` lists in file order; the unit body — path,
//!    `base_values`, complex, `parent_unit`, `enemy_unit` (nested units
//!    first appear here);
//! 3. the remaining units (outside lists) — at the end, so nothing is lost.
//!
//! For a reference file loaded by [`GameState::from_save`], such a traversal
//! repeats the original numbers **byte for byte** — this is checked by
//! `tests/save_write.rs::save1_load_to_save_is_byte_identical`.
//!
//! # What is not recovered from runtime (honest defaults)
//!
//! * `markers[10]`, `screen_locations[6]`, `selected_unit`, `zoom_level`,
//!   `camera_position`, `display_button_*` — `TeamInfo` UI fields (state-model
//!   §6.3); C++ keeps them in `CTInfo`, our `GameState` — in
//!   [`SaveMeta::team_ui`] (transferred from the loaded save; when creating
//!   a game — `CTInfo::Reset` defaults).
//! * `AiPlayer::unused_v70`/`target_team_v70` — V70 fields that C++
//!   **reads and discards** (`aiplayer.cpp`); the values are transferred via
//!   [`SaveMeta`] so that the round-trip is byte-exact.
//! * `stats_gold_spent_on_upgrades` — C++ clamps ≥ 0 on load; the raw
//!   `int16_t` is in [`SaveMeta::team_gold_spent_raw`].
//! * Heat maps: C++ `HeatMap::LoadV70` clamps negatives to 0, and in
//!   memory a cell is `uint32_t`; back it is written as `min(127, value)` as `i8`.
//! * `MessageLog` — the UI log; transferred from [`SaveMeta::message_logs`].
//! * `movement_wait_count` (not part of V70), `experience` (for
//!   `COMMANDO`/`REGENERATING_UNIT` packed into `storage`), `transfer_cargo` and
//!   `stealth_dice_roll` (V70 stores them in `move_to_grid`) — not written, as
//!   in C++.

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

/// `Hash_UnitHash` size in the V70 file (`hash.cpp`: 512 buckets in SAVE1/SAVE2).
pub const SAVE_UNIT_HASH_SIZE: u16 = 512;

/// V70 data that the simulation does not use but that is needed to write
/// a 1:1 save (P3).
///
/// Lives in [`GameState::save_meta`]. On load it is filled from the file
/// ([`SaveMeta::from_save`]), when creating a game — from settings
/// ([`SaveMeta::default`] + `max_app` fields). It does **not** enter the canonical
/// state digest (`GameState::snapshot_bytes`): it is not game state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveMeta {
    /// Header `save_game_type` (`FileType`).
    pub save_game_type: FileType,
    /// `save_name[30]` — the save name as raw bytes.
    pub save_name: [u8; 30],
    /// `header.world` — the game's planet.
    pub world: Planet,
    /// `team_name[4][30]` — team names.
    pub team_names: [[u8; 30]; 4],
    /// `rng_seed` — the game's RNG seed (V70 does not store the `Randomizer` state).
    pub rng_seed: u32,
    /// `header.opponent` — AI difficulty.
    pub opponent: Opponent,
    /// `turn_timer_time` — turn timer, seconds.
    pub turn_timer_time: u16,
    /// `endturn_time` — end-of-turn timer, seconds.
    pub endturn_time: u16,
    /// `IniOptions` (12 × `signed int`).
    pub ini_options: IniOptions,
    /// `IniPreferences` (7 × `signed int`).
    pub ini_preferences: IniPreferences,
    /// `TeamInfo` UI fields × 4 (`markers`/`screen_locations`/camera/zoom/buttons).
    pub team_ui: [TeamInfoUi; 4],
    /// Raw `stats_gold_spent_on_upgrades` × 4 (V70 `int16_t`).
    pub team_gold_spent_raw: [i16; 4],
    /// Header `team_type[PLAYER_TEAM_ALIEN]` (V70 writes the 5th slot, `CTInfo`
    /// does not store it).
    pub alien_team_type: TeamKind,
    /// Header `team_clan[PLAYER_TEAM_ALIEN]`.
    pub alien_team_clan: TeamClan,
    /// `AiPlayer::unused_v70` × 4 — C++ reads and discards it.
    pub ai_unused_v70: [i16; 4],
    /// `AiPlayer::target_team` × 4 (V70) — C++ reads and discards it.
    pub ai_target_team_v70: [i16; 4],
    /// Whether the team AI's `info_map` block was in the file (`ReadObjectCount != 0`).
    ///
    /// Reference V70 saves have no AI maps (C++ writes `item_count = 0`), while
    /// the `max_core` runtime always keeps a 112×112 map — without this flag writing
    /// would add 12,544 bytes per computer team.
    pub ai_has_info_map: [bool; 4],
    /// Whether the `mine_map` block was in the file.
    pub ai_has_mine_map: [bool; 4],
    /// `UnitHash` order as in the file: buckets → `UnitId`.
    ///
    /// C++ inserts units into a bucket on creation (`UnitsManager_DeployUnit`),
    /// so the order within a bucket is creation order, which at runtime cannot
    /// be derived from ids or lists. We transfer it from the loaded save,
    /// and append new units to the end of the bucket.
    pub unit_hash_order: Vec<Vec<UnitId>>,
    /// Message logs × 4 (`MessageLogList`).
    pub message_logs: [Vec<MessageLogEntry>; 4],
    /// Remaining turn timer in `GameManagerState` (seconds).
    pub turn_timer: u16,
    /// `Hash_UnitHash` size in the file (C++ 512; in the `max_core` runtime — 8192).
    pub unit_hash_size: u16,
    /// `Hash_MapHash` size in the file (original V70 saves — 512).
    pub map_hash_size: u16,
    /// `x_shift` from the `MapHash` header (2 for 512 buckets).
    pub map_hash_x_shift: u16,
    /// `MapHash` cells in file order: `(x, y, units)`.
    ///
    /// On load C++ **rehashes** the table to the current map size
    /// (`hash.cpp:240`), so the original bucket order cannot
    /// be derived from runtime; it is transferred here for byte-exact writing. On save
    /// the order is preserved for unchanged cells, new ones are appended to
    /// the end ([`build_save_file`]).
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
    /// Extracts metadata from a parsed V70 file.
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
        // The file's `UnitHash` references objects → convert to `UnitId`.
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

    /// Save name as a string.
    pub fn save_name(&self) -> String {
        fixed_name(&self.save_name)
    }

    /// Team name as a string.
    pub fn team_name(&self, team: usize) -> String {
        self.team_names
            .get(team)
            .map(|name| fixed_name(name))
            .unwrap_or_default()
    }

    /// Writes a string into a 30-byte name field (NUL terminator, truncation).
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
// Object layout
// ---------------------------------------------------------------------------

/// V70 object key: what exactly the object is in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ObjectKey {
    /// Team `UnitValues`: base or current values of a type.
    TeamValues {
        /// Team.
        team: u8,
        /// `false` — current values, `true` — base.
        base: bool,
        /// Unit type index 0..92.
        unit_type: u16,
    },
    /// A `Complex` from the `TeamUnits::complexes` list.
    TeamComplex {
        /// Team.
        team: u8,
        /// `Complex::id`.
        id: i16,
    },
    /// `UnitValues` of a concrete unit (`UnitInfo::base_values`).
    UnitValues {
        /// The unit's `UnitId`.
        id: u16,
    },
    /// Unit path (`AirPath`/`BuilderPath`/`GroundPath`).
    UnitPath {
        /// The unit's `UnitId`.
        id: u16,
    },
    /// A `Complex` referenced by a unit but absent from the team list
    /// (corrupt save/desync): a minimal body is written.
    OrphanComplex {
        /// Team.
        team: u8,
        /// `Complex::id`.
        id: i16,
    },
    /// `UnitInfo`.
    Unit {
        /// The unit's `UnitId`.
        id: u16,
    },
}

/// Assignment of object numbers in traversal order (C++ `SmartFileWriter`).
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

    /// Object number for a key; a new object gets the next number.
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

    /// Number of an already assigned object.
    fn get(&self, key: ObjectKey) -> Option<u16> {
        self.objects.get(&key).copied()
    }

    /// `UnitInfo` object number (0 — no unit).
    fn unit(&self, id: Option<UnitId>) -> u16 {
        id.and_then(|id| self.get(ObjectKey::Unit { id: id.0 }))
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Assembling SaveFile
// ---------------------------------------------------------------------------

/// Assembles the V70 representation of live state.
///
/// All objects are renumbered in the C++ writer's traversal order (see the module
/// documentation), so a reference save loaded and immediately written
/// comes out byte-identical (see `tests/save_write.rs`).
pub fn build_save_file(state: &GameState) -> SaveFile {
    let meta = &state.save_meta;
    let mut layout = Layout::new();

    // --- 1. TeamUnits × 4: values and complexes ---
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

        // C++ `TeamUnits::FileSave` writes **all base** values first, then
        // all current ones — object numbers are assigned in this same order.
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
            // C++ `TeamUnits` holds `UnitValues*`: while there has been no research,
            // current and base are one object. At runtime only values remain,
            // so sharing is restored by equality: an upgrade always
            // changes at least one characteristic (`upgrade_offset_factor`).
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
            // V70 writes `signed short`; C++ stores `uint32_t`.
            gold: units.gold as u16,
            base_values,
            current_values,
            complexes,
        };
    }

    // --- 2-3. Units: lists and bodies ---
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

    // Units that are in no list (for example, just created and
    // not yet added to a list): written as reference 0 they would not get into the lists, but
    // the object must be in the file, otherwise it would be lost.
    for id in state.unit_ids_in_order() {
        walk_unit(state, id, &mut layout, &mut units, 0);
    }

    units.sort_by_key(|unit| unit.object_index);

    // --- UnitHash: bucket order as in the loaded file + new units ---
    //
    // C++ inserts a unit into a bucket on creation (`UnitsManager_DeployUnit`),
    // so the order within a bucket is creation order: it is not derived from
    // ids/lists and is transferred via [`SaveMeta::unit_hash_order`].
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

    // --- MapHash: cell order as in the file + new cells ---
    //
    // Size and x_shift come from [`SaveMeta`]: C++ rehashes on load
    // the table to the map size, so the reference's 512 buckets cannot
    // be restored from runtime. Cells that were in the file are written in the old order with
    // the **current** set of units; cells that appeared in the game — at the end.
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
            // The cell became empty — no object in the file (C++ deletes empty ones).
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

    // --- heat maps × 4 (playing teams only) ---
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

    // --- message logs × 4 ---
    let mut message_logs: [Vec<MessageLogEntry>; SAVE_TEAM_COUNT] =
        std::array::from_fn(|_| Vec::new());
    for (team, logs) in message_logs.iter_mut().enumerate() {
        for entry in &meta.message_logs[team] {
            let mut entry = entry.clone();
            entry.unit_object_index = layout.unit(entry.unit);
            logs.push(entry);
        }
    }

    // --- AiPlayer × 4 (computer teams only) ---
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

    // --- header and settings ---
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

/// Heat-map cell → the file's `int8_t` (C++ `HeatMap::FileSave` writes
/// the `legacy` value; in memory a cell is `uint32_t`).
fn heat_value(value: u32) -> i8 {
    value.min(i8::MAX as u32) as i8
}

/// `info_map` (112×112) → the file array; a short map is padded
/// `INFO_MAP_NO_INFO` (0).
fn map_bytes(values: &[u8]) -> Option<Box<[u8; MAP_TILES]>> {
    let mut map = Box::new([0u8; MAP_TILES]);
    for (slot, value) in map.iter_mut().zip(values.iter()) {
        *slot = *value;
    }
    Some(map)
}

/// `mine_map` (112×112) → the file array.
fn mine_map_bytes(values: &[i8]) -> Option<Box<[i8; MAP_TILES]>> {
    let mut map = Box::new([0i8; MAP_TILES]);
    for (slot, value) in map.iter_mut().zip(values.iter()) {
        *slot = *value;
    }
    Some(map)
}

/// `UnitValues` object number for the unit's `base_values`.
///
/// In C++ `UnitInfo::base_values` is a pointer to the team's `UnitValues` (base
/// or current — depending on when the unit was created), so the object is usually
/// **shared** with `TeamUnits`. At runtime only values remain, so
/// the restoration rule is: the unit's values equal the team's base → the base
/// object, equal the current → the current, otherwise (an upgrade not in the table) —
/// the unit's own object. For SAVE1/SAVE2 the rule matches the file
/// (checked by `tests/save_write.rs`).
fn unit_values_object(state: &GameState, info: &UnitInfo, layout: &mut Layout) -> u16 {
    let team = info.team.0 as usize;
    let unit_type = info.unit_type as usize;
    // Only the four playing teams' `TeamUnits` exist in the file. The derelict
    // team (`PLAYER_TEAM_ALIEN`) is a service team without a `TeamUnits` section,
    // so sharing a derelict unit's values with its live table would leave a
    // dangling object number; C++ writes such values as a separate object on
    // first reference.
    if team < SAVE_TEAM_COUNT {
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
    }

    layout.object(ObjectKey::UnitValues { id: info.id.0 })
}

/// Writes a unit body and its nested objects; returns the object number.
///
/// Nested object order — as in C++ `UnitInfo::FileSave`: path,
/// `base_values`, complex, `parent_unit`, `enemy_unit`.
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
    // Presentation is stored next to the unit in `GameState` (F5.3/B) and written to
    // V70 in the original byte order (`save::write_unit_body`).
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
                    // The complex is lost (the unit references a nonexistent id):
                    // write a minimal body so the reference stays valid.
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
    /// V70 representation of the state (P3): object numbering is reassembled
    /// in the C++ writer's order, see [`build_save_file`].
    pub fn to_save(&self) -> SaveFile {
        build_save_file(self)
    }

    /// V70 save bytes of the current game.
    pub fn to_save_bytes(&self) -> Result<Vec<u8>, crate::save::SaveError> {
        self.to_save().to_bytes()
    }

    /// Save settings/service fields (V70).
    pub fn save_meta(&self) -> &SaveMeta {
        &self.save_meta
    }

    /// Replaces the save settings/service fields (used when creating
    /// a game and when saving from the UI).
    pub(crate) fn set_save_meta(&mut self, meta: SaveMeta) {
        self.save_meta = meta;
    }

    /// Appends a message to the team log (`MessageLogList`): it enters
    /// the V70 save, not the state digest.
    pub fn push_message(&mut self, team: u8, text: &str, point: Point, unit: Option<UnitId>) {
        let index = usize::from(team).min(SAVE_TEAM_COUNT - 1);
        let mut bytes = text.as_bytes().to_vec();
        // V70 writes `strlen + 1` — text with a terminating NUL.
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
        // The "model -> file -> model" cycle without units is stable.
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

    /// Writer object order: numbers are consecutive in traversal order.
    ///
    /// Catches the "base and current values mixed up" regression (C++ writes
    /// all base `UnitValues` first, then all current ones), which made
    /// `SaveFile::to_bytes` fail with `ForwardReference`.
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
        // Unit numbers are unique, come after team objects, and together with
        // all nested objects form a continuous range 1..=N
        // (required by `SmartFileWriter`).
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

    /// `SaveMeta::default` matches the V70 initialization of a new game: four
    /// 512-bucket tables, `Custom` file type, no RNG seed, snow planet.
    #[test]
    fn save_meta_defaults_match_v70() {
        let meta = SaveMeta::default();
        assert_eq!(meta.save_game_type, FileType::Custom);
        assert_eq!(meta.world, Planet::Snowcrab);
        assert_eq!(meta.rng_seed, 0);
        assert_eq!(meta.opponent, Opponent::Average);
        assert_eq!(meta.unit_hash_size, SAVE_UNIT_HASH_SIZE);
        assert_eq!(meta.map_hash_size, SAVE_UNIT_HASH_SIZE);
        assert_eq!(meta.map_hash_x_shift, 2);
        assert_eq!(meta.alien_team_type, TeamKind::None);
        assert_eq!(meta.alien_team_clan, TeamClan::Random);
        assert_eq!(meta.turn_timer, 0);
        assert!(meta.unit_hash_order.is_empty());
        assert!(meta.map_hash_cells.is_empty());
        assert!(meta.save_name().is_empty());
        assert!(meta.team_name(0).is_empty());
        assert!(meta.team_name(9).is_empty());
        assert!(meta.message_logs.iter().all(Vec::is_empty));
    }

    /// `Layout` numbers objects from 1 and saturates at `u16::MAX` instead of
    /// wrapping (a game cannot reach 65535 objects, but the counter must not
    /// silently reuse a number).
    #[test]
    fn layout_numbers_saturate_at_u16_max() {
        let mut layout = Layout::new();
        for id in 0..u16::MAX {
            let index = layout.object(ObjectKey::Unit { id });
            assert_eq!(index, id.wrapping_add(1), "unit {id}");
            // Re-requesting an existing key returns the same number.
            assert_eq!(layout.object(ObjectKey::Unit { id }), index);
        }
        let last = layout.object(ObjectKey::Unit { id: u16::MAX });
        assert_eq!(last, u16::MAX);
        // The counter is saturated: a further object reuses the last number
        // (the writer then rejects the file with `ForwardReference`, it never
        // wraps around to 1).
        assert_eq!(layout.object(ObjectKey::Unit { id: 0 }), 1);
        assert_eq!(
            layout.object(ObjectKey::Unit {
                id: u16::MAX - 1
            }),
            u16::MAX
        );
        assert_eq!(
            layout.object(ObjectKey::Unit {
                id: u16::MAX - 2
            }),
            u16::MAX - 1
        );
    }
}
