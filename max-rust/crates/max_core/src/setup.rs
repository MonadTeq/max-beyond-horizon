//! Детерминированная генерация карты при старте партии (фаза P2).
//!
//! # Что именно портировано (и откуда)
//!
//! В оракуле `max-cpp-port/src` **нет** файла `setup.cpp`: генерация карты
//! разложена по трём местам, и это зафиксировано здесь явно.
//!
//! | Шаг `GameManager_InitGame()` | Файл оракула | Что делает | Порт |
//! |---|---|---|---|
//! | `ResourceManager_InitInGameAssets(world)` | `world.cpp` (LoadTilesAndMetadata) | строит `SurfaceMap` из `.wrl`-файла планеты: `surface[i] = PassData[pass_table[tile_ids[i]]]` | [`derive_surface_map`] — чистая функция от `tile_ids`/`pass_table` |
//! | `Ai_SetupStrategy(team)` (для компьютеров) | `aiplayer.cpp` | тратит RNG, но **до** пересеивания | [`crate::ai::ai_setup_strategy`] (P6: только выбор стратегии, без генерации карты) |
//! | `GameManager_PopulateMapWithResources()` | `game_manager.cpp:4730` + `resource_allocator.cpp` | `Randomizer_SetSeed(Remote_RngSeed)`, 3×`PopulateCargoMap`, `Optimize` у стартовых позиций команд, `SeparateResources`, 3×`ConcentrateResources`, `SettleMinimumResourceLevels` | [`populate_map_with_resources`] |
//! | `GameManager_PopulateMapWithAlienUnits(...)` | `game_manager.cpp:4942` | сетка точек (RNG), `SpawnAlienDerelicts` (RNG: здание, мосты, юниты) | [`populate_map_with_alien_units`] |
//! | `GameManager_InitMap()` | `game_manager.cpp:1531` | только UI (зум/рендер) | не портируется |
//! | `GameManager_ProcessTeamMissionSupplyUnits(team)` | `game_manager.cpp:4967` | высадка юнитов, `Survey_GetTotalResourcesInArea` | вне P2 (нужны таблицы юнитов, P3) |
//!
//! Отдельно про «surfaces по типу планеты»: **процедурной генерации поверхности в
//! порте нет**. Номер планеты (`settings["world"]`) выбирает ресурс-файл `.wrl`,
//! из которого `World::LoadTilesAndMetadata()` читает карту tile-id и pass-table и
//! переводит её в байты `SurfaceMap`. Соответственно в Rust поверхность — вход
//! [`SurfaceMap`], а портирован только её вывод из tile-data ([`derive_surface_map`]),
//! который и сверяется векторами. `World::ApplyBugFixes()` (правки конкретных
//! `.wrl` по SHA-256) — тоже данные ассетов, а не логика генерации.
//!
//! Про `menu_update_resource_levels()` (`menu.cpp:866`): настройки `raw_resource`,
//! `fuel_resource`, `gold_resource`, `alien_derelicts` из меню (0–2) разворачиваются
//! в числовые параметры генерации (`raw_normal_low`…, `max_resources`,
//! `min_resources`, `alien_seperation`, `alien_unit_value`). Это портировано как
//! [`GenerationSettings::from_menu_levels`] — иначе «уровни ресурсов 0–2» в
//! `resource_allocator.cpp` не воспроизвести.
//!
//! # Детерминизм
//!
//! Все циклы и все обращения к [`Randomizer`] идут в порядке C++ 1:1:
//! `resource_allocator.cpp` (`GetValue` — два вызова на тайл; `SeparateResources` —
//! два `Generate(5)` и т.д.), `GameManager_PopulateMapWithAlienUnits` и
//! `GameManager_SpawnAlienDerelicts` (`Randomizer_Generate(std::size(...))` — ветка
//! `size_t`, т.е. [`Randomizer::generate_u64`]). Тест `matches_cpp_reference_vectors`
//! сверяет не только карты, но и **итоговое состояние RNG** — это ловит любое
//! расхождение в числе/порядке вызовов.
//!
//! # Ограничения (осознанные)
//!
//! * Стоимость юнитов (`ATTRIB_TURNS`) для alien-обломков в P2 ещё не портирована
//!   (таблицы атрибутов — P3): она приходит входом [`AlienUnitCosts`] и берётся из
//!   векторов. Ветвление и порядок RNG при этом портированы полностью.
//! * `Access_GetModifiedSurfaceType()` на этапе генерации не видит юнитов: карта
//!   ещё пуста, поэтому для стартовых позиций она равна `SurfaceMap`. Для
//!   alien-обломков порт учитывает единственный тип юнита, который меняет
//!   проходимость, — мосты (`BRIDGE` → клетка воды/побережья становится `LAND`),
//!   как в `access.cpp:950`.
//! * `UnitsManager_DeployUnit()` не пишет в `CargoMap`/`SurfaceMap` (проверено по
//!   `units_manager.cpp:2354` и `unitinfo.cpp:1301`), поэтому расстановка обломков
//!   портирована как запись [`AlienPlacement`] без остального состояния юнитов.

use crate::enums::{Planet, Surface, UnitType, CARGO_GOLD, CARGO_MATERIALS};
use crate::geo::{Point, DIRECTION_OFFSETS, MAP_HEIGHT, MAP_TILES, MAP_WIDTH};
use crate::map::{Map, ResourceKind, ResourceMap, SurfaceMap};
use crate::rng::Randomizer;
use crate::team::TeamId;

/// `PassData[]` из `world.cpp:382` — индекс pass-таблицы → бит `SurfaceType`.
pub const PASS_TABLE_SURFACES: [Surface; 4] =
    [Surface::LAND, Surface::WATER, Surface::COAST, Surface::AIR];

/// `GameManager_AlienBuildings[]` (`game_manager.cpp:136`).
pub const ALIEN_BUILDINGS: [UnitType; 3] = [
    UnitType::ShieldGenerator,
    UnitType::SupportPlant,
    UnitType::RecreationCenter,
];

/// `GameManager_AlienUnits[]` (`game_manager.cpp:138`).
pub const ALIEN_UNITS: [UnitType; 3] = [
    UnitType::AlienTank,
    UnitType::AlienAssaultGun,
    UnitType::AlienPlane,
];

/// Уровень ресурсов/обломков из меню — C++ `settings["raw_resource"]` и т.п.
///
/// Допустимы только 0, 1, 2 (`menu_update_resource_levels` имеет `switch` без
/// `default`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceLevel(u8);

impl ResourceLevel {
    /// Уровень 0 (`case 0` в `menu_update_resource_levels`).
    pub const LOW: Self = Self(0);
    /// Уровень 1 (`case 1`).
    pub const NORMAL: Self = Self(1);
    /// Уровень 2 (`case 2`).
    pub const HIGH: Self = Self(2);

    /// Создаёт уровень; `None` вне диапазона 0..=2.
    #[inline]
    pub const fn new(value: u8) -> Option<Self> {
        if value <= 2 {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Значение уровня (0..=2).
    #[inline]
    pub const fn value(self) -> u8 {
        self.0
    }
}

/// Параметры генерации одного типа ресурса — `settings["raw_normal_low"]` и т.п.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceKindSettings {
    /// `*_normal_low`.
    pub normal_low: i32,
    /// `*_normal_high`.
    pub normal_high: i32,
    /// `*_concentrate_low`.
    pub concentrate_low: i32,
    /// `*_concentrate_high`.
    pub concentrate_high: i32,
    /// `*_concentrate_seperation` (так в C++, с опечаткой).
    pub concentrate_seperation: i32,
    /// `*_concentrate_diffusion`.
    pub concentrate_diffusion: i32,
}

/// Числовые параметры генерации — ровно те `Settings->GetNumericValue(...)`,
/// которые читают `resource_allocator.cpp` и `game_manager.cpp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenerationSettings {
    /// `max_resources`.
    pub max_resources: i32,
    /// `min_resources`.
    pub min_resources: i32,
    /// `mixed_resource_seperation`.
    pub mixed_resource_seperation: i32,
    /// Параметры материалов (`CARGO_MATERIALS`).
    pub raw: ResourceKindSettings,
    /// Параметры топлива (`CARGO_FUEL`).
    pub fuel: ResourceKindSettings,
    /// Параметры золота (`CARGO_GOLD`).
    pub gold: ResourceKindSettings,
    /// `alien_seperation`.
    pub alien_seperation: i32,
    /// `alien_unit_value`.
    pub alien_unit_value: i32,
}

impl GenerationSettings {
    /// Порт `menu_update_resource_levels()` (`menu.cpp:866`).
    ///
    /// Разворачивает четыре настройки меню (0–2) в числовые параметры генерации,
    /// включая перекрёстную `mixed_resource_seperation` (зависит от `raw + fuel`)
    /// и итоговые `min_resources`/`max_resources`.
    ///
    /// Для дефолтных уровней 1/1/1/0 получаются значения
    /// `min_resources = 9`, `max_resources = 20` — как в `settings.cpp`.
    pub fn from_menu_levels(
        raw: ResourceLevel,
        fuel: ResourceLevel,
        gold: ResourceLevel,
        derelicts: ResourceLevel,
    ) -> Self {
        // `resource_min = 1`, `resource_max = 15` из `menu.cpp`, затем case-ветки
        // переписывают их (raw) или инкрементируют (fuel, gold).
        let (raw_settings, mut resource_min, mut resource_max) = match raw.value() {
            0 => (
                ResourceKindSettings {
                    normal_low: 0,
                    normal_high: 3,
                    concentrate_low: 8,
                    concentrate_high: 12,
                    concentrate_seperation: 28,
                    concentrate_diffusion: 6,
                },
                4,
                16,
            ),
            1 => (
                ResourceKindSettings {
                    normal_low: 0,
                    normal_high: 5,
                    concentrate_low: 13,
                    concentrate_high: 16,
                    concentrate_seperation: 23,
                    concentrate_diffusion: 5,
                },
                6,
                17,
            ),
            _ => (
                ResourceKindSettings {
                    normal_low: 1,
                    normal_high: 5,
                    concentrate_low: 16,
                    concentrate_high: 16,
                    concentrate_seperation: 19,
                    concentrate_diffusion: 4,
                },
                7,
                18,
            ),
        };

        let fuel_settings = match fuel.value() {
            0 => {
                resource_min += 2;
                resource_max += 1;

                ResourceKindSettings {
                    normal_low: 1,
                    normal_high: 2,
                    concentrate_low: 8,
                    concentrate_high: 12,
                    concentrate_seperation: 29,
                    concentrate_diffusion: 6,
                }
            }
            1 => {
                resource_min += 3;
                resource_max += 2;

                ResourceKindSettings {
                    normal_low: 2,
                    normal_high: 3,
                    concentrate_low: 12,
                    concentrate_high: 16,
                    concentrate_seperation: 24,
                    concentrate_diffusion: 5,
                }
            }
            _ => {
                resource_min += 4;
                resource_max += 3;

                ResourceKindSettings {
                    normal_low: 2,
                    normal_high: 4,
                    concentrate_low: 16,
                    concentrate_high: 16,
                    concentrate_seperation: 20,
                    concentrate_diffusion: 4,
                }
            }
        };

        let mixed_resource_seperation = match raw.value() + fuel.value() {
            0 => 49,
            1 => 44,
            2 => 40,
            3 => 36,
            _ => 33,
        };

        let gold_settings = match gold.value() {
            0 => ResourceKindSettings {
                normal_low: 0,
                normal_high: 0,
                concentrate_low: 5,
                concentrate_high: 9,
                concentrate_seperation: 39,
                concentrate_diffusion: 6,
            },
            1 => {
                resource_max += 1;

                ResourceKindSettings {
                    normal_low: 0,
                    normal_high: 0,
                    concentrate_low: 8,
                    concentrate_high: 12,
                    concentrate_seperation: 32,
                    concentrate_diffusion: 5,
                }
            }
            _ => {
                resource_min += 1;
                resource_max += 2;

                ResourceKindSettings {
                    normal_low: 0,
                    normal_high: 1,
                    concentrate_low: 12,
                    concentrate_high: 16,
                    concentrate_seperation: 26,
                    concentrate_diffusion: 4,
                }
            }
        };

        let (alien_seperation, alien_unit_value) = match derelicts.value() {
            0 => (0, 0),
            1 => (65, 20),
            _ => (50, 30),
        };

        Self {
            max_resources: resource_max,
            min_resources: resource_min,
            mixed_resource_seperation,
            raw: raw_settings,
            fuel: fuel_settings,
            gold: gold_settings,
            alien_seperation,
            alien_unit_value,
        }
    }

    /// Параметры конкретного типа ресурса.
    #[inline]
    pub fn kind(&self, kind: ResourceKind) -> &ResourceKindSettings {
        match kind {
            ResourceKind::Raw => &self.raw,
            ResourceKind::Fuel => &self.fuel,
            ResourceKind::Gold => &self.gold,
        }
    }
}

/// Стартовая позиция команды — `UnitsManager_TeamMissionSupplies[team].starting_position`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeamStart {
    /// Команда (`PLAYER_TEAM_RED..GRAY`).
    pub team: TeamId,
    /// Стартовая позиция (мутируется `find_valid_starting_position`).
    pub position: Point,
}

impl TeamStart {
    /// Создаёт запись о стартовой позиции.
    #[inline]
    pub const fn new(team: TeamId, position: Point) -> Self {
        Self { team, position }
    }
}

/// Стоимость alien-юнитов в ходах — `UnitValues::GetAttribute(ATTRIB_TURNS)`.
///
/// В P2 таблицы атрибутов (93×14) ещё не портированы, поэтому значения приходят
/// входом. Для команды пришельцев `current_values == base_values`, поэтому одна
/// таблица покрывает оба использования в `GameManager_SpawnAlienDerelicts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlienUnitCosts {
    /// `ALNTANK` — он же `cost` в `GameManager_SpawnAlienDerelicts`.
    pub alien_tank: i32,
    /// `JUGGRNT`.
    pub juggernaut: i32,
    /// `ALNASGUN`.
    pub alien_assault_gun: i32,
    /// `ALNPLANE`.
    pub alien_plane: i32,
}

impl AlienUnitCosts {
    /// `ATTRIB_TURNS` юнита; паникует для типов, которые генерация не запрашивает
    /// (здание/мост/плита: их стоимость в C++ не читается).
    #[inline]
    pub fn turns(&self, unit: UnitType) -> i32 {
        match unit {
            UnitType::AlienTank => self.alien_tank,
            UnitType::Juggernaut => self.juggernaut,
            UnitType::AlienAssaultGun => self.alien_assault_gun,
            UnitType::AlienPlane => self.alien_plane,
            other => panic!("AlienUnitCosts::turns: unrequested unit type {other:?}"),
        }
    }
}

/// Один вызов `UnitsManager_DeployUnit` при генерации alien-обломков.
///
/// Порядок записей — порядок развёртывания в C++ (плита → здание → мосты →
/// юниты), поэтому он же порядок вызовов RNG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlienPlacement {
    /// Тип юнита/здания.
    pub unit_type: UnitType,
    /// Клетка развёртывания.
    pub position: Point,
}

/// Вход генерации партии: планета, настройки, команды и стоимость обломков.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSetup {
    /// Планета (`settings["world"]`); выбор `.wrl`-ассета — вне крейта.
    pub planet: Planet,
    /// Числовые параметры генерации (из уровней меню).
    pub settings: GenerationSettings,
    /// Активные команды **в порядке `PLAYER_TEAM_RED..GRAY`** со стартовыми
    /// позициями (как `GameManager_InitGame` обходит команды).
    pub teams: Vec<TeamStart>,
    /// Стоимости alien-юнитов.
    pub alien_costs: AlienUnitCosts,
}

/// Результат генерации: карта и расстановка alien-обломков.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupOutput {
    /// Карта: поверхность + ресурсы.
    pub map: Map,
    /// Развёрнутые обломки (в порядке `UnitsManager_DeployUnit`).
    pub aliens: Vec<AlienPlacement>,
}

// ---------------------------------------------------------------------------
// SurfaceMap из tile-data (.wrl): world.cpp:382-409
// ---------------------------------------------------------------------------

/// Строит [`SurfaceMap`] из tile-id карты и pass-таблицы — дословный порт
/// `World::LoadTilesAndMetadata()` (`world.cpp:382-409`).
///
/// ```c
/// const uint8_t PassData[] = {LAND, WATER, COAST, AIR};
/// for (i = 0; i < tile_count; ++i) pass_table[i] = PassData[pass_table[i]];
/// for (i = 0; i < map_cell_count; ++i) surface[i] = pass_table[tile_ids[i]];
/// ```
///
/// Некорректные индексы (вне `pass_table`/`PassData`) в C++ — выход за границы
/// массива; здесь они дают [`Surface::NONE`] и фиксируются `debug_assert`.
pub fn derive_surface_map(tile_ids: &[u16], pass_table: &[u8]) -> SurfaceMap {
    let mut tiles = [Surface::NONE; MAP_TILES];
    let mut table = Vec::with_capacity(pass_table.len());

    for &raw_index in pass_table {
        debug_assert!(
            (raw_index as usize) < PASS_TABLE_SURFACES.len(),
            "pass table entry {raw_index} is out of PassData bounds"
        );
        table.push(
            PASS_TABLE_SURFACES
                .get(raw_index as usize)
                .copied()
                .unwrap_or(Surface::NONE),
        );
    }

    let cells = tile_ids.len().min(MAP_TILES);

    for (index, slot) in tiles.iter_mut().take(cells).enumerate() {
        let tile_id = tile_ids[index] as usize;

        // В C++ `pass_table[tile_ids[i]]` — чтение вне границ pass-таблицы;
        // здесь такой тайл остаётся `NONE` (данные `.wrl` всегда валидны).
        *slot = table.get(tile_id).copied().unwrap_or(Surface::NONE);
    }

    SurfaceMap::from_tiles(tiles)
}

// ---------------------------------------------------------------------------
// ResourceAllocator — resource_allocator.cpp
// ---------------------------------------------------------------------------

/// `ResourceRange` (`resource_allocator.cpp:29`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResourceRange {
    min: i32,
    max: i32,
}

impl ResourceRange {
    /// `((Generate(max-min+1)+min) + (Generate(max-min+1)+min) + 1) / 2`.
    ///
    /// Порядок двух вызовов RNG в C++ не определён (операнды `+`), но оба
    /// слагаемых складываются, поэтому сумма от порядка не зависит.
    #[inline]
    fn value(self, rng: &mut Randomizer) -> i32 {
        let span = self.max - self.min + 1;
        let first = rng.generate_i32(span) + self.min;
        let second = rng.generate_i32(span) + self.min;

        (first + second + 1) / 2
    }
}

/// `ResourceAllocator` (`resource_allocator.cpp`).
struct ResourceAllocator {
    material_type: u16,
    material_point: Point,
    normal: ResourceRange,
    concentrate: ResourceRange,
    concentrate_seperation: i32,
    concentrate_variance: i32,
    concentrate_diffusion: i32,
    max_resources: i32,
    mixed_resource_seperation: i32,
    mixed_resource_seperation_variance: i32,
}

impl ResourceAllocator {
    /// Конструктор `ResourceAllocator(uint16_t material_type)`.
    fn new(settings: &GenerationSettings, kind: ResourceKind) -> Self {
        let material_point = match kind {
            ResourceKind::Raw => Point::new(0, 0),
            ResourceKind::Gold => Point::new(1, 0),
            ResourceKind::Fuel => Point::new(1, 1),
        };
        let kind_settings = settings.kind(kind);

        Self {
            material_type: kind.cargo_bits(),
            material_point,
            normal: ResourceRange {
                min: kind_settings.normal_low,
                max: kind_settings.normal_high,
            },
            concentrate: ResourceRange {
                min: kind_settings.concentrate_low,
                max: kind_settings.concentrate_high,
            },
            concentrate_seperation: kind_settings.concentrate_seperation,
            concentrate_variance: kind_settings.concentrate_seperation / 5,
            concentrate_diffusion: kind_settings.concentrate_diffusion,
            max_resources: settings.max_resources,
            mixed_resource_seperation: settings.mixed_resource_seperation,
            mixed_resource_seperation_variance: settings.mixed_resource_seperation / 5,
        }
    }

    /// `GetZoneResoureLevel(grid_x, grid_y)` — сумма `amount` по зоне 2×2.
    fn zone_resource_level(cargo: &ResourceMap, grid_x: i32, grid_y: i32) -> i32 {
        let width = MAP_WIDTH as i32;
        let height = MAP_HEIGHT as i32;

        let mut ulx = grid_x;
        let mut uly = grid_y;
        let mut lrx = grid_x + 2;
        let mut lry = grid_y + 2;

        if ulx < 0 {
            ulx = 0;
        }

        if uly < 0 {
            uly = 0;
        }

        if width < lrx {
            lrx = width;
        }

        if height < lry {
            lry = height;
        }

        let mut result = 0;

        for i in ulx..lrx {
            for j in uly..lry {
                result += (cargo.bits_at((width * j + i) as usize) & 0x1F) as i32;
            }
        }

        result
    }

    /// `OptimizeResources(grid_x, grid_y, level_min, level_max)`.
    fn optimize_resources(
        cargo: &ResourceMap,
        grid_x: i32,
        grid_y: i32,
        mut level_min: i32,
        level_max: i32,
    ) -> i32 {
        let zone_resource_level = Self::zone_resource_level(cargo, grid_x, grid_y);

        if (zone_resource_level + level_min) > level_max {
            level_min = level_max - zone_resource_level;
        }

        if level_min < 0 {
            level_min = 0;
        }

        level_min
    }

    /// `Optimize(Point point, resource_level, resource_value)`.
    fn optimize(
        &self,
        cargo: &mut ResourceMap,
        point: Point,
        mut resource_level: i32,
        resource_value: i32,
    ) {
        let width = MAP_WIDTH as i32;
        let height = MAP_HEIGHT as i32;
        let point_x = (((((point.x as i32) + 1) - self.material_point.x as i32) / 2) * 2)
            + self.material_point.x as i32;
        let point_y = (((((point.y as i32) + 1) - self.material_point.y as i32) / 2) * 2)
            + self.material_point.y as i32;

        // bounds: uly = lry в C++-структуре Rect; здесь — отдельные локальные
        // переменные, чтобы не тянуть UI-тип Rect в симуляцию.
        let mut bounds_lry = point_y - 4;

        if bounds_lry < 0 {
            bounds_lry = 0;
        }

        bounds_lry = ((bounds_lry / 2) * 2) + self.material_point.y as i32;

        let mut bounds_lrx = point_x - 4;

        if bounds_lrx < 0 {
            bounds_lrx = 0;
        }

        bounds_lrx = ((bounds_lrx / 2) * 2) + self.material_point.x as i32;

        let mut bounds_uly = point_y + 5;

        if bounds_uly > height {
            bounds_uly = height;
        }

        let mut bounds_ulx = point_x + 5;

        if bounds_ulx > width {
            bounds_ulx = width;
        }

        let map_index = point_y * width + point_x;
        let map_resource_amount = if map_index >= 0 && map_index < width * height {
            (cargo.bits_at(map_index as usize) & 0x1F) as i32
        } else {
            0
        };

        if point_x > 0 {
            if point_y > 0 {
                resource_level = Self::optimize_resources(
                    cargo,
                    point_x - 1,
                    point_y - 1,
                    resource_level,
                    resource_value + map_resource_amount,
                );
            }

            if point_y < height - 1 {
                resource_level = Self::optimize_resources(
                    cargo,
                    point_x - 1,
                    point_y,
                    resource_level,
                    resource_value + map_resource_amount,
                );
            }
        }

        if point_x < width - 1 {
            if point_y > 0 {
                resource_level = Self::optimize_resources(
                    cargo,
                    point_x,
                    point_y - 1,
                    resource_level,
                    resource_value + map_resource_amount,
                );
            }

            if point_y < height - 1 {
                resource_level = Self::optimize_resources(
                    cargo,
                    point_x,
                    point_y,
                    resource_level,
                    resource_value + map_resource_amount,
                );
            }
        }

        resource_level *= 100;

        let mut grid_x = bounds_lrx;

        while grid_x < bounds_ulx {
            let mut grid_y = bounds_lry;

            while grid_y < bounds_uly {
                let distance_factor = ((approximate_distance(point_x - grid_x, point_y - grid_y)
                    * 10)
                    / self.concentrate_diffusion)
                    + 10;
                let material_value = resource_level / (distance_factor * distance_factor);
                let index = (grid_y * width + grid_x) as usize;

                if material_value > (cargo.bits_at(index) & 0x1F) as i32 {
                    cargo.set_bits_at(index, (self.material_type as i32 + material_value) as u16);
                }

                grid_y += 2;
            }

            grid_x += 2;
        }
    }

    /// `PopulateCargoMap()` — нормальные (не концентрированные) залежи.
    fn populate_cargo_map(&self, cargo: &mut ResourceMap, rng: &mut Randomizer) {
        let width = MAP_WIDTH as i32;
        let height = MAP_HEIGHT as i32;
        let mut i = self.material_point.x as i32;

        while i < width {
            let mut j = self.material_point.y as i32;

            while j < height {
                let value = self.normal.value(rng);
                let index = (width * j + i) as usize;

                cargo.set_bits_at(index, (self.material_type as i32 + value) as u16);

                j += 2;
            }

            i += 2;
        }
    }

    /// `ConcentrateResources()`.
    fn concentrate_resources(&self, cargo: &mut ResourceMap, rng: &mut Randomizer) {
        let width = MAP_WIDTH as i32;
        let height = MAP_HEIGHT as i32;
        let point1_x = rng.generate_i32((self.concentrate_seperation * 4) / 5 + 1);
        let point1_y = rng.generate_i32(self.concentrate_seperation / 2 + 1);
        let mut flag = false;
        let mut i = point1_x;

        while i < width {
            let mut j = if flag {
                self.concentrate_seperation / 2 + point1_y
            } else {
                point1_y
            };

            while j < height {
                let point2_x = rng.generate_i32(self.concentrate_variance * 2 + 1)
                    - self.concentrate_variance
                    + i;
                let point2_y = rng.generate_i32(self.concentrate_variance * 2 + 1)
                    - self.concentrate_variance
                    + j;

                self.optimize(
                    cargo,
                    Point::new(point2_x as i16, point2_y as i16),
                    self.concentrate.value(rng),
                    self.max_resources,
                );

                j += self.concentrate_seperation;
            }

            flag = !flag;
            i += (self.concentrate_seperation * 4) / 5;
        }
    }

    /// `SeparateResources(ResourceAllocator* allocator)` — размежевание
    /// материалов и топлива.
    fn separate_resources(&self, allocator: &Self, cargo: &mut ResourceMap, rng: &mut Randomizer) {
        let width = MAP_WIDTH as i32;
        let height = MAP_HEIGHT as i32;
        let point1_x = rng.generate_i32(((self.mixed_resource_seperation * 4) / 5) + 1);
        let point1_y = rng.generate_i32(self.mixed_resource_seperation / 2 + 1);
        let mut flag = false;
        let mut i = point1_x;

        while i < width {
            let mut j = if flag {
                self.mixed_resource_seperation / 2 + point1_y
            } else {
                point1_y
            };

            while j < height {
                let point2_x = rng.generate_i32(self.mixed_resource_seperation_variance * 2 + 1)
                    - self.mixed_resource_seperation_variance
                    + i;
                let point2_y = rng.generate_i32(self.mixed_resource_seperation_variance * 2 + 1)
                    - self.mixed_resource_seperation_variance
                    + j;
                let point2 = Point::new(point2_x as i16, point2_y as i16);

                self.optimize(cargo, point2, rng.generate_i32(5) + 8, self.max_resources);
                allocator.optimize(
                    cargo,
                    point2,
                    rng.generate_i32(5) + 8,
                    allocator.max_resources,
                );

                j += self.mixed_resource_seperation;
            }

            flag = !flag;
            i += (self.mixed_resource_seperation * 4) / 5;
        }
    }

    /// `SettleMinimumResourceLevels(int32_t min_level)` (статический в C++).
    fn settle_minimum_resource_levels(cargo: &mut ResourceMap, min_level: i32) {
        let width = MAP_WIDTH as i32;
        let height = MAP_HEIGHT as i32;

        for i in 0..width {
            for j in 0..height {
                if Self::zone_resource_level(cargo, i - 1, j - 1) < min_level
                    && Self::zone_resource_level(cargo, i - 1, j) < min_level
                    && Self::zone_resource_level(cargo, i, j - 1) < min_level
                    && Self::zone_resource_level(cargo, i, j) < min_level
                {
                    cargo.set_bits_at((width * j + i) as usize, CARGO_MATERIALS as u16);
                }
            }
        }
    }
}

/// `Access_GetApproximateDistance(distance_x, distance_y)` (`access.cpp:386`).
#[inline]
pub fn approximate_distance(distance_x: i32, distance_y: i32) -> i32 {
    // C++: labs(); диапазон координат карты далёк от i32::MIN.
    let distance_x = distance_x.abs();
    let distance_y = distance_y.abs();

    if distance_x >= distance_y {
        2 * distance_x + distance_y
    } else {
        2 * distance_y + distance_x
    }
}

// ---------------------------------------------------------------------------
// Стартовые позиции: game_manager.cpp
// ---------------------------------------------------------------------------

/// `GameManager_IsValidStartingPosition(grid_x, grid_y)` (`game_manager.cpp:4196`).
///
/// На этапе генерации карта пуста, поэтому
/// `Access_GetModifiedSurfaceType()` == `World::GetSurfaceType()`, и проверка
/// сводится к «все 16 клеток 4×4 вокруг — ровно `LAND`».
pub fn is_valid_starting_position(surface: &SurfaceMap, grid_x: i32, grid_y: i32) -> bool {
    if grid_x < 1
        || grid_y < 1
        || (grid_x + 3) > MAP_WIDTH as i32
        || (grid_y + 3) > MAP_HEIGHT as i32
    {
        return false;
    }

    for x in (grid_x - 1)..(grid_x + 3) {
        for y in (grid_y - 1)..(grid_y + 3) {
            if surface.get(x as i16, y as i16) != Surface::LAND {
                return false;
            }
        }
    }

    true
}

/// `GameManager_FindValidStartingPosition(Point* position)` (`game_manager.cpp:4712`).
///
/// Мутирует позицию; если валидных клеток нет вообще — цикл не завершится
/// (как в C++). `range` наращивается на 2 каждый виток.
pub fn find_valid_starting_position(surface: &SurfaceMap, position: &mut Point) {
    if is_valid_starting_position(surface, position.x as i32, position.y as i32) {
        return;
    }

    let mut range = 2i32;

    loop {
        position.x = (position.x as i32 - 1) as i16;
        position.y = (position.y as i32 + 1) as i16;

        for direction in (0..8usize).step_by(2) {
            for _ in 0..range {
                *position += DIRECTION_OFFSETS[direction];

                if is_valid_starting_position(surface, position.x as i32, position.y as i32) {
                    return;
                }
            }
        }

        range += 2;
    }
}

// ---------------------------------------------------------------------------
// Ресурсы: GameManager_PopulateMapWithResources
// ---------------------------------------------------------------------------

/// Порт `GameManager_PopulateMapWithResources()` (`game_manager.cpp:4730`).
///
/// Порядок RNG-вызовов 1:1 с C++:
/// 1. `PopulateCargoMap` для материалов, топлива, золота (в этом порядке);
/// 2. для каждой активной команды (в порядке возрастания индекса):
///    `FindValidStartingPosition`, затем `Optimize` материалов (`minimum_materials`,
///    100) и топлива (`minimum_fuel`, 100);
/// 3. `materials.SeparateResources(&fuel)`;
/// 4. `ConcentrateResources` — материалы, топливо, золото;
/// 5. `SettleMinimumResourceLevels(min_resources)`.
///
/// C++ пересеивает RNG в начале функции (`Randomizer_SetSeed(Remote_RngSeed)`);
/// Rust-аналог — вызывающий код создаёт [`Randomizer::new(seed)`] непосредственно
/// перед вызовом (см. [`generate_map`]).
pub fn populate_map_with_resources(
    settings: &GenerationSettings,
    surface: &SurfaceMap,
    teams: &mut [TeamStart],
    rng: &mut Randomizer,
) -> ResourceMap {
    let materials = ResourceAllocator::new(settings, ResourceKind::Raw);
    let fuel = ResourceAllocator::new(settings, ResourceKind::Fuel);
    let gold = ResourceAllocator::new(settings, ResourceKind::Gold);

    let max_resources = settings.max_resources;
    let minimum_fuel = (max_resources * 8 + 10) / 20;
    let minimum_materials = (max_resources * 12 + 10) / 20;

    let mut cargo = ResourceMap::new();

    materials.populate_cargo_map(&mut cargo, rng);
    fuel.populate_cargo_map(&mut cargo, rng);
    gold.populate_cargo_map(&mut cargo, rng);

    for team in teams.iter_mut() {
        find_valid_starting_position(surface, &mut team.position);
        materials.optimize(&mut cargo, team.position, minimum_materials, 100);
        fuel.optimize(&mut cargo, team.position, minimum_fuel, 100);
    }

    materials.separate_resources(&fuel, &mut cargo, rng);

    materials.concentrate_resources(&mut cargo, rng);
    fuel.concentrate_resources(&mut cargo, rng);
    gold.concentrate_resources(&mut cargo, rng);

    ResourceAllocator::settle_minimum_resource_levels(&mut cargo, settings.min_resources);

    cargo
}

// ---------------------------------------------------------------------------
// Alien-обломки: GameManager_PopulateMapWithAlienUnits / SpawnAlienDerelicts
// ---------------------------------------------------------------------------

/// Состояние клеток, покрытых мостами (`BRIDGE`).
///
/// `Access_GetModifiedSurfaceType()` (`access.cpp:950`) сообщает `LAND` для
/// воды/побережья, на которых стоит мост, — единственный эффект юнитов, который
/// видит генерация.
struct BridgeCells {
    cells: [bool; MAP_TILES],
}

impl BridgeCells {
    fn new() -> Self {
        Self {
            cells: [false; MAP_TILES],
        }
    }

    fn mark(&mut self, position: Point) {
        if let Some(index) = SurfaceMap::index(position.x, position.y) {
            self.cells[index] = true;
        }
    }

    /// `Access_GetModifiedSurfaceType(x, y, false)` без учёта остальных юнитов.
    fn modified_surface_type(&self, surface: &SurfaceMap, position: Point) -> Surface {
        let surface_type = surface.get(position.x, position.y);

        if surface_type == Surface::WATER || surface_type == Surface::COAST {
            if let Some(index) = SurfaceMap::index(position.x, position.y) {
                if self.cells[index] {
                    return Surface::LAND;
                }
            }
        }

        surface_type
    }
}

/// `Access_IsInsideBounds(&bounds{0,0,W,H}, point)`.
#[inline]
fn is_inside_bounds(position: Point) -> bool {
    position.x >= 0
        && (position.x as i32) < MAP_WIDTH as i32
        && position.y >= 0
        && (position.y as i32) < MAP_HEIGHT as i32
}

/// `GameManager_FindSpot(Point* point)` (`game_manager.cpp:4763`).
fn find_spot(point: &mut Point, surface: &SurfaceMap, teams: &[TeamStart], bridges: &BridgeCells) {
    let width = MAP_WIDTH as i32;
    let height = MAP_HEIGHT as i32;
    let mut range = 2i32;

    loop {
        point.x = (point.x as i32 - 1) as i16;
        point.y = (point.y as i32 + 1) as i16;

        for direction in (0..8usize).step_by(2) {
            for _ in 0..range {
                *point += DIRECTION_OFFSETS[direction];

                if point.x >= 1
                    && point.y >= 1
                    && (point.x as i32 + 3) < width
                    && (point.y as i32 + 3) < height
                {
                    let mut is_found = true;

                    for team in teams {
                        if team.position.distance_squared(Point::new(point.x, point.y)) < 256 {
                            is_found = false;
                        }
                    }

                    let mut grid_x = point.x as i32;

                    while grid_x < point.x as i32 + 2 && is_found {
                        let mut grid_y = point.y as i32;

                        while grid_y < point.y as i32 + 2 && is_found {
                            if bridges.modified_surface_type(
                                surface,
                                Point::new(grid_x as i16, grid_y as i16),
                            ) == Surface::AIR
                            {
                                is_found = false;
                            }

                            grid_y += 1;
                        }

                        grid_x += 1;
                    }

                    if is_found {
                        return;
                    }
                }
            }
        }

        range += 2;
    }
}

/// `GameManager_SpawnAlienDerelicts(Point point, int32_t alien_unit_value)`
/// (`game_manager.cpp:4805`).
///
/// `UnitsManager_DeployUnit` здесь заменён записью в `placements` (побочных
/// эффектов на карту ресурсов у него нет), `BRIDGE` дополнительно помечает
/// клетку как проходимую сушу для последующих проверок.
fn spawn_alien_derelicts(
    point: Point,
    alien_unit_value: i32,
    surface: &SurfaceMap,
    teams: &[TeamStart],
    costs: &AlienUnitCosts,
    bridges: &mut BridgeCells,
    placements: &mut Vec<AlienPlacement>,
    rng: &mut Randomizer,
) {
    let mut point = point;
    let mut position = Point::ZERO;

    find_spot(&mut point, surface, teams, bridges);

    placements.push(AlienPlacement {
        unit_type: UnitType::LargeSlab,
        position: point,
    });

    // `Randomizer_Generate(std::size(GameManager_AlienBuildings))` — ветка `size_t`.
    let building_index = rng.generate_u64(ALIEN_BUILDINGS.len() as u64) as usize;

    placements.push(AlienPlacement {
        unit_type: ALIEN_BUILDINGS[building_index],
        position: point,
    });

    position.x = (point.x as i32 - 1) as i16;
    position.y = (point.y as i32 + 2) as i16;

    for direction in (0..8usize).step_by(2) {
        for _ in 0..3 {
            position += DIRECTION_OFFSETS[direction];

            if is_inside_bounds(position) {
                let surface_type = surface.get(position.x, position.y);

                if surface_type == Surface::WATER || surface_type == Surface::COAST {
                    placements.push(AlienPlacement {
                        unit_type: UnitType::Bridge,
                        position,
                    });

                    bridges.mark(position);
                }
            }
        }
    }

    let cost = costs.turns(UnitType::AlienTank);
    let mut alien_unit_value = alien_unit_value;
    let mut offset = 2i32;

    while alien_unit_value >= cost && offset > 0 {
        let mut land_tiles: Vec<Point> = Vec::new();
        let mut water_tiles: Vec<Point> = Vec::new();

        position.x = (point.x as i32 - offset) as i16;
        position.y = (point.y as i32 + offset + 1) as i16;

        for direction in (0..8usize).step_by(2) {
            for _ in 0..(2 * offset + 1) {
                position += DIRECTION_OFFSETS[direction];

                if is_inside_bounds(position) {
                    let surface_type = bridges.modified_surface_type(surface, position);

                    if surface_type == Surface::WATER {
                        water_tiles.push(position);
                    } else if surface_type == Surface::LAND {
                        land_tiles.push(position);
                    }
                }
            }
        }

        let mut value = if alien_unit_value < costs.turns(UnitType::Juggernaut) {
            1
        } else {
            0
        };

        if alien_unit_value < value {
            water_tiles.clear();
        }

        while alien_unit_value >= cost && (land_tiles.len() + water_tiles.len()) > 0 {
            let total = (land_tiles.len() + water_tiles.len()) as u32;
            let deployed;

            if rng.generate(total) + 1 <= water_tiles.len() as u32 {
                debug_assert!(!water_tiles.is_empty());

                let index = rng.generate(water_tiles.len() as u32) as usize;

                position = water_tiles.remove(index);
                deployed = UnitType::Juggernaut;
            } else {
                debug_assert!(!land_tiles.is_empty());

                let index = rng.generate(land_tiles.len() as u32) as usize;

                position = land_tiles.remove(index);

                // `do { idx = Generate(size(AlienUnits)); } while (turns > value)`.
                let alien_unit_index = loop {
                    let candidate = rng.generate_u64(ALIEN_UNITS.len() as u64) as usize;

                    if costs.turns(ALIEN_UNITS[candidate]) <= alien_unit_value {
                        break candidate;
                    }
                };

                deployed = ALIEN_UNITS[alien_unit_index];
            }

            placements.push(AlienPlacement {
                unit_type: deployed,
                position,
            });

            alien_unit_value -= costs.turns(deployed);

            value = if alien_unit_value < costs.turns(UnitType::Juggernaut) {
                1
            } else {
                0
            };

            if alien_unit_value < value {
                water_tiles.clear();
            }
        }

        offset -= 1;
    }
}

/// Порт `GameManager_PopulateMapWithAlienUnits(alien_seperation, alien_unit_value)`
/// (`game_manager.cpp:4942`).
///
/// Сетка точек обходится с чередованием `flag` (как в C++), внутри каждой точки —
/// [`spawn_alien_derelicts`]. При `alien_unit_value == 0` (обломки выключены)
/// функция не делает ни одного вызова RNG.
pub fn populate_map_with_alien_units(
    settings: &GenerationSettings,
    surface: &SurfaceMap,
    teams: &[TeamStart],
    costs: &AlienUnitCosts,
    rng: &mut Randomizer,
) -> Vec<AlienPlacement> {
    let mut placements = Vec::new();

    if settings.alien_unit_value > 0 {
        let alien_seperation = settings.alien_seperation;
        let alien_unit_value = settings.alien_unit_value;
        let mut bridges = BridgeCells::new();
        let point1_x = rng.generate_i32((alien_seperation * 4) / 5 + 1);
        let point1_y = rng.generate_i32(alien_seperation / 2 + 1);
        let variance = ((alien_seperation / 5) - ((-alien_seperation) / 5)) + 1;
        let offset = (-alien_seperation) / 5;
        let mut flag = false;
        let mut i = point1_x;

        while i < MAP_WIDTH as i32 {
            let mut j = if flag {
                alien_seperation / 2 + point1_y
            } else {
                point1_y
            };

            while j < MAP_HEIGHT as i32 {
                let point2_x = rng.generate_i32(variance) + offset + i;
                let point2_y = rng.generate_i32(variance) + offset + j;

                spawn_alien_derelicts(
                    Point::new(point2_x as i16, point2_y as i16),
                    alien_unit_value,
                    surface,
                    teams,
                    costs,
                    &mut bridges,
                    &mut placements,
                    rng,
                );

                j += alien_seperation;
            }

            flag = !flag;
            i += (alien_seperation * 4) / 5;
        }
    }

    placements
}

// ---------------------------------------------------------------------------
// Оркестратор
// ---------------------------------------------------------------------------

/// Полная генерация карты при старте партии — симуляционная часть
/// `GameManager_InitGame()` (`game_manager.cpp:3056`) в порядке вызовов.
///
/// Соответствие C++:
/// ```c
/// GameManager_PopulateMapWithResources();                      // seed ставится внутри
/// GameManager_PopulateMapWithAlienUnits(alien_seperation, alien_unit_value);
/// ```
///
/// `rng` должен быть только что сидирован (`Randomizer::new(seed)`) — это
/// эквивалент `Randomizer_SetSeed(Remote_RngSeed)` в начале
/// `GameManager_PopulateMapWithResources`.
///
/// Функция чистая: единственные входы — `setup`, `surface` и состояние `rng`;
/// единственные выходы — [`SetupOutput`] и продвинутый `rng`.
pub fn generate_map(
    setup: &mut GameSetup,
    surface: &SurfaceMap,
    rng: &mut Randomizer,
) -> SetupOutput {
    let resources = populate_map_with_resources(&setup.settings, surface, &mut setup.teams, rng);
    let aliens = populate_map_with_alien_units(
        &setup.settings,
        surface,
        &setup.teams,
        &setup.alien_costs,
        rng,
    );

    SetupOutput {
        map: Map::from_parts(surface.clone(), resources),
        aliens,
    }
}

/// `CARGO_GOLD` в виде `u16` — вспомогательная константа для читаемости тестов.
pub const CARGO_GOLD_BITS: u16 = CARGO_GOLD as u16;

// ---------------------------------------------------------------------------
// Тесты
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::ResourceMapEntry;

    /// Эталонные векторы, сгенерированные C++-программой
    /// `max-rust/tools/map_vectors/main.cpp` (см. README рядом с `vectors.txt`).
    const VECTORS: &str = include_str!("../../../tools/map_vectors/vectors.txt");

    const MAP_CELLS: usize = MAP_TILES;

    fn hex_to_bytes(text: &str) -> Vec<u8> {
        assert_eq!(text.len() % 2, 0, "hex string has odd length");
        let bytes = text.as_bytes();

        (0..bytes.len() / 2)
            .map(|index| {
                let pair =
                    std::str::from_utf8(&bytes[index * 2..index * 2 + 2]).expect("ascii hex");
                u8::from_str_radix(pair, 16).expect("valid hex byte")
            })
            .collect()
    }

    fn hex_to_u16(text: &str) -> Vec<u16> {
        let bytes = hex_to_bytes(text);
        assert_eq!(bytes.len() % 2, 0, "u16 hex string has odd byte count");

        bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect()
    }

    #[derive(Debug)]
    struct VectorCase {
        name: String,
        planet: u8,
        levels: [u8; 4],
        seed: u32,
        starts: Vec<(u8, i16, i16)>,
        pass_table: Vec<u8>,
        tile_ids: Vec<u16>,
        surface: Vec<u8>,
        alien_costs: [i32; 4],
        resources: Vec<u16>,
        placements: Vec<(u16, i16, i16)>,
        rng_state: [u32; 4],
    }

    fn parse_cases(text: &str) -> Vec<VectorCase> {
        let mut cases: Vec<VectorCase> = Vec::new();

        for line in text.lines() {
            let line = line.trim();

            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let mut parts = line.split_whitespace();
            let key = parts.next().expect("record key");

            if key == "case" {
                cases.push(VectorCase {
                    name: parts.next().unwrap_or_default().to_owned(),
                    planet: 0,
                    levels: [0; 4],
                    seed: 0,
                    starts: Vec::new(),
                    pass_table: Vec::new(),
                    tile_ids: Vec::new(),
                    surface: Vec::new(),
                    alien_costs: [0; 4],
                    resources: Vec::new(),
                    placements: Vec::new(),
                    rng_state: [0; 4],
                });

                continue;
            }

            let case = cases.last_mut().expect("vector record before `case`");

            match key {
                "planet" => {
                    case.planet = parts.next().expect("planet").parse().expect("planet id");
                }
                "levels" => {
                    for (slot, value) in case.levels.iter_mut().zip(parts) {
                        *slot = value.parse().expect("resource level");
                    }
                }
                "seed" => {
                    case.seed = parts.next().expect("seed").parse().expect("seed value");
                }
                "start" => {
                    let team: u8 = parts.next().expect("team").parse().expect("team id");
                    let x: i16 = parts.next().expect("x").parse().expect("start x");
                    let y: i16 = parts.next().expect("y").parse().expect("start y");

                    case.starts.push((team, x, y));
                }
                "pass_table" => {
                    case.pass_table = hex_to_bytes(parts.next().expect("pass table"));
                }
                "tile_ids" => {
                    case.tile_ids = hex_to_u16(parts.next().expect("tile ids"));
                }
                "surface" => {
                    case.surface = hex_to_bytes(parts.next().expect("surface"));
                }
                "alien_costs" => {
                    for (slot, value) in case.alien_costs.iter_mut().zip(parts) {
                        *slot = value.parse().expect("alien cost");
                    }
                }
                "resources" => {
                    case.resources = hex_to_u16(parts.next().expect("resources"));
                }
                "placement" => {
                    let unit_type: u16 =
                        parts.next().expect("unit type").parse().expect("unit type");
                    let x: i16 = parts.next().expect("x").parse().expect("placement x");
                    let y: i16 = parts.next().expect("y").parse().expect("placement y");

                    case.placements.push((unit_type, x, y));
                }
                "rng_state" => {
                    for (slot, value) in case.rng_state.iter_mut().zip(parts) {
                        *slot = value.parse::<u32>().expect("rng state word (decimal)");
                    }
                }
                other => panic!("unknown vector record: {other}"),
            }
        }

        cases
    }

    fn level(value: u8) -> ResourceLevel {
        ResourceLevel::new(value).expect("menu resource levels are 0..=2")
    }

    fn build_setup(case: &VectorCase) -> GameSetup {
        GameSetup {
            planet: Planet::try_from(case.planet).expect("planet id in range"),
            settings: GenerationSettings::from_menu_levels(
                level(case.levels[0]),
                level(case.levels[1]),
                level(case.levels[2]),
                level(case.levels[3]),
            ),
            teams: case
                .starts
                .iter()
                .map(|(team, x, y)| TeamStart::new(TeamId(*team), Point::new(*x, *y)))
                .collect(),
            alien_costs: AlienUnitCosts {
                alien_tank: case.alien_costs[0],
                juggernaut: case.alien_costs[1],
                alien_assault_gun: case.alien_costs[2],
                alien_plane: case.alien_costs[3],
            },
        }
    }

    /// Сверка карты поверхности с диагностикой по первой различающейся клетке
    /// (полный дамп 12 544 байт в сообщении бесполезен).
    fn assert_surface_matches(case: &str, got: &[u8], want: &[u8]) {
        assert_eq!(
            got.len(),
            want.len(),
            "surface size mismatch in case {case}"
        );

        for (index, (got, want)) in got.iter().zip(want.iter()).enumerate() {
            assert_eq!(
                got,
                want,
                "surface mismatch in case {case} at (x={}, y={}): got 0x{got:02X}, expected 0x{want:02X}",
                index % MAP_WIDTH as usize,
                index / MAP_WIDTH as usize
            );
        }
    }

    /// Полная сверка бит-в-бит с C++: поверхность из tile-data, карта ресурсов,
    /// расстановка alien-обломков и итоговое состояние RNG.
    #[test]
    fn matches_cpp_reference_vectors() {
        let cases = parse_cases(VECTORS);

        assert!(
            cases.len() >= 6,
            "expected at least 6 vector cases, got {}",
            cases.len()
        );

        let mut cmp_tiles = 0usize;
        let mut placements = 0usize;

        for case in &cases {
            assert_eq!(
                case.surface.len(),
                MAP_CELLS,
                "surface size, case {}",
                case.name
            );
            assert_eq!(
                case.resources.len(),
                MAP_CELLS,
                "resource map size, case {}",
                case.name
            );

            // 1. SurfaceMap: порт world.cpp (pass table → surface map).
            let surface = derive_surface_map(&case.tile_ids, &case.pass_table);

            assert_surface_matches(&case.name, &surface.to_bytes(), &case.surface);

            cmp_tiles += MAP_CELLS;

            // 2. Генерация ресурсов + alien-обломков с тем же сидом.
            let mut setup = build_setup(case);
            let mut rng = Randomizer::new(case.seed);
            let output = generate_map(&mut setup, &surface, &mut rng);

            let resources = output.map.resources.to_le_bytes();

            for (index, (got, want)) in resources
                .chunks_exact(2)
                .zip(case.resources.iter())
                .enumerate()
            {
                let got = u16::from_le_bytes([got[0], got[1]]);

                assert_eq!(
                    got,
                    *want,
                    "resource map mismatch in case {} at index {} (x={}, y={})",
                    case.name,
                    index,
                    index % MAP_WIDTH as usize,
                    index / MAP_WIDTH as usize
                );
            }

            cmp_tiles += MAP_CELLS;

            // 3. Alien-обломки: полный порядок развёртывания.
            assert_eq!(
                output.aliens.len(),
                case.placements.len(),
                "alien placement count mismatch in case {}",
                case.name
            );

            for (index, (got, want)) in output.aliens.iter().zip(case.placements.iter()).enumerate()
            {
                assert_eq!(
                    (got.unit_type as u16, got.position.x, got.position.y),
                    *want,
                    "alien placement {index} mismatch in case {}",
                    case.name
                );
            }

            placements += output.aliens.len();

            // 4. Состояние RNG: ловит расхождение числа/порядка вызовов.
            assert_eq!(
                rng.state(),
                case.rng_state,
                "rng state mismatch in case {}",
                case.name
            );
        }

        assert_eq!(cmp_tiles, MAP_CELLS * 2 * cases.len());
        assert!(placements > 0, "vectors must exercise alien derelicts");
    }

    /// Генерация детерминирована: два прогона с одним сидом дают одно и то же
    /// состояние (карта, обломки, RNG).
    #[test]
    fn generation_is_deterministic() {
        let cases = parse_cases(VECTORS);
        let case = &cases[0];
        let surface = derive_surface_map(&case.tile_ids, &case.pass_table);

        let mut first_setup = build_setup(case);
        let mut first_rng = Randomizer::new(case.seed);
        let first = generate_map(&mut first_setup, &surface, &mut first_rng);

        let mut second_setup = build_setup(case);
        let mut second_rng = Randomizer::new(case.seed);
        let second = generate_map(&mut second_setup, &surface, &mut second_rng);

        assert_eq!(
            first.map.resources.to_le_bytes(),
            second.map.resources.to_le_bytes()
        );
        assert_eq!(first.aliens, second.aliens);
        assert_eq!(first_rng.state(), second_rng.state());
        assert_eq!(first_setup.teams, second_setup.teams);
    }

    /// Ресурсы остаются в границах битфилда: количество ≤ 15 по маске, тип —
    /// ровно один бит, разведанность генерацией не выставляется.
    #[test]
    fn generated_resources_stay_within_bitfield() {
        let cases = parse_cases(VECTORS);

        for case in &cases {
            let surface = derive_surface_map(&case.tile_ids, &case.pass_table);
            let mut setup = build_setup(case);
            let mut rng = Randomizer::new(case.seed);
            let output = generate_map(&mut setup, &surface, &mut rng);

            for entry in output.map.resources.tiles() {
                let bits = entry.bits();

                assert_eq!(
                    bits & !(0x3FFFu16),
                    0,
                    "unexpected bits outside amount/type/visibility in case {}: 0x{bits:04X}",
                    case.name
                );
                assert!(
                    entry.amount() <= 0x1F,
                    "amount out of mask in case {}",
                    case.name
                );
                assert!(
                    !(entry.bits() & 0x0F != 0) || entry.resource_type().is_some(),
                    "amount without type in case {}",
                    case.name
                );
                assert_eq!(
                    entry.visibility(),
                    0,
                    "generation must not set team visibility (case {})",
                    case.name
                );
            }

            let counts = output.map.resources.counts();

            assert!(counts.raw_tiles > 0, "no materials in case {}", case.name);
            assert!(counts.fuel_tiles > 0, "no fuel in case {}", case.name);
            assert!(counts.gold_tiles > 0, "no gold in case {}", case.name);
            assert!(
                counts.raw_amount <= 0x1F * counts.raw_tiles,
                "material amounts out of range in case {}",
                case.name
            );
        }
    }

    /// `derive_surface_map` — дословная семантика `world.cpp:382-409`:
    /// сначала pass-индексы заменяются на `PassData[]`, затем ими индексируются
    /// tile-id.
    #[test]
    fn derive_surface_map_follows_pass_data_order() {
        // PassData = {LAND, WATER, COAST, AIR}; pass_table remap is identity here:
        // pass 0→LAND, 1→WATER, 2→COAST, 3→AIR.
        let pass_table = [0u8, 1, 2, 3];
        let mut tile_ids = vec![0u16; MAP_TILES];

        tile_ids[0] = 0; // LAND
        tile_ids[1] = 1; // WATER
        tile_ids[2] = 2; // COAST
        tile_ids[3] = 3; // AIR

        let surface = derive_surface_map(&tile_ids, &pass_table);

        assert_eq!(surface.get(0, 0), Surface::LAND);
        assert_eq!(surface.get(1, 0), Surface::WATER);
        assert_eq!(surface.get(2, 0), Surface::COAST);
        assert_eq!(surface.get(3, 0), Surface::AIR);
        assert_eq!(surface.get(4, 0), Surface::LAND, "tile 0 index defaults");

        // Remap: pass 0 → AIR, pass 1 → COAST, pass 2 → WATER, pass 3 → LAND.
        let surface = derive_surface_map(&tile_ids, &[3u8, 2, 1, 0]);

        assert_eq!(surface.get(0, 0), Surface::AIR);
        assert_eq!(surface.get(1, 0), Surface::COAST);
        assert_eq!(surface.get(2, 0), Surface::WATER);
        assert_eq!(surface.get(3, 0), Surface::LAND);

        // Tile id outside the pass table stays NONE (C++ reads out of bounds).
        let surface = derive_surface_map(&tile_ids, &[0u8]);

        assert_eq!(surface.get(0, 0), Surface::LAND, "pass 0 is LAND");
        assert_eq!(
            surface.get(1, 0),
            Surface::NONE,
            "tile id outside pass table is NONE (C++ reads out of bounds)"
        );
        assert_eq!(surface.get(5, 0), Surface::LAND, "tile id 0 stays LAND");
    }

    /// `from_menu_levels` повторяет `menu_update_resource_levels()`: уровни 1/1/1/0
    /// дают дефолтные `min_resources = 9`, `max_resources = 20`.
    #[test]
    fn menu_levels_match_cpp_settings() {
        let settings = GenerationSettings::from_menu_levels(
            ResourceLevel::NORMAL,
            ResourceLevel::NORMAL,
            ResourceLevel::NORMAL,
            ResourceLevel::LOW,
        );

        assert_eq!(settings.max_resources, 20);
        assert_eq!(settings.min_resources, 9);
        assert_eq!(settings.mixed_resource_seperation, 40);
        assert_eq!(settings.raw.normal_high, 5);
        assert_eq!(settings.fuel.concentrate_seperation, 24);
        assert_eq!(settings.gold.concentrate_diffusion, 5);
        assert_eq!(settings.alien_seperation, 0);
        assert_eq!(settings.alien_unit_value, 0);

        // Крайние уровни: 0/0/0 и 2/2/2.
        let low = GenerationSettings::from_menu_levels(
            ResourceLevel::LOW,
            ResourceLevel::LOW,
            ResourceLevel::LOW,
            ResourceLevel::HIGH,
        );
        // raw=0 → (4,16); fuel=0 → +2/+1; gold=0 → без изменений.
        assert_eq!((low.min_resources, low.max_resources), (6, 17));
        assert_eq!(low.mixed_resource_seperation, 49);
        assert_eq!((low.alien_seperation, low.alien_unit_value), (50, 30));

        let high = GenerationSettings::from_menu_levels(
            ResourceLevel::HIGH,
            ResourceLevel::HIGH,
            ResourceLevel::HIGH,
            ResourceLevel::NORMAL,
        );
        // raw=2 → (7,18); fuel=2 → +4/+3; gold=2 → +1/+2.
        assert_eq!((high.min_resources, high.max_resources), (12, 23));
        assert_eq!(high.mixed_resource_seperation, 33);
        assert_eq!((high.alien_seperation, high.alien_unit_value), (65, 20));
    }

    /// `find_valid_starting_position` двигается ровно по спирали
    /// `game_manager.cpp:4712` и находит первый LAND-квадрат 4×4.
    #[test]
    fn find_valid_starting_position_walks_spiral() {
        let mut surface = SurfaceMap::new();
        let mut position = Point::new(10, 10);

        // Всё — вода, кроме квадрата 4×4 в точке (20, 20).
        for tile in surface.tiles_mut().iter_mut() {
            *tile = Surface::WATER;
        }

        for x in 19..=22 {
            for y in 19..=22 {
                surface.set(x, y, Surface::LAND);
            }
        }

        find_valid_starting_position(&surface, &mut position);

        assert!(is_valid_starting_position(
            &surface,
            position.x as i32,
            position.y as i32
        ));
        assert_eq!(position, Point::new(20, 20), "spiral order must match C++");
    }

    /// `is_valid_starting_position` требует ровно `LAND` (побережье не годится).
    #[test]
    fn starting_position_requires_pure_land() {
        let mut surface = SurfaceMap::new();

        for x in 0..MAP_WIDTH {
            for y in 0..MAP_HEIGHT {
                surface.set(x, y, Surface::LAND);
            }
        }

        assert!(is_valid_starting_position(&surface, 1, 1));
        assert!(is_valid_starting_position(&surface, 108, 108));
        assert!(!is_valid_starting_position(&surface, 0, 1));
        // C++: `grid_x + 3 > MapSize.x` → 109 всё ещё допустимо, 110 — уже нет.
        assert!(is_valid_starting_position(&surface, 109, 1));
        assert!(!is_valid_starting_position(&surface, 110, 1));

        surface.set(2, 2, Surface::LAND | Surface::COAST);
        assert!(
            !is_valid_starting_position(&surface, 1, 1),
            "coast is not land for starting positions"
        );
    }

    /// Без обломков (`alien_unit_value == 0`) генерация не тратит RNG и не
    /// разворачивает юнитов.
    #[test]
    fn alien_population_disabled_consumes_no_rng() {
        let settings = GenerationSettings::from_menu_levels(
            ResourceLevel::NORMAL,
            ResourceLevel::NORMAL,
            ResourceLevel::NORMAL,
            ResourceLevel::LOW,
        );
        let surface = derive_surface_map(&vec![3u16; MAP_TILES], &[0u8, 1, 2, 3]);
        let costs = AlienUnitCosts {
            alien_tank: 4,
            juggernaut: 8,
            alien_assault_gun: 5,
            alien_plane: 6,
        };
        let mut rng = Randomizer::new(7);
        let before = rng.state();
        let placements = populate_map_with_alien_units(&settings, &surface, &[], &costs, &mut rng);

        assert!(placements.is_empty());
        assert_eq!(rng.state(), before);
    }

    /// `ResourceMapEntry`/`ResourceMap` сохраняют раскладку после записи
    /// генератора (`CargoMap[i] = type + amount`).
    #[test]
    fn resource_bits_are_written_like_cpp() {
        let settings = GenerationSettings::from_menu_levels(
            ResourceLevel::NORMAL,
            ResourceLevel::NORMAL,
            ResourceLevel::NORMAL,
            ResourceLevel::LOW,
        );
        let mut surface = SurfaceMap::new();

        for x in 10..101 {
            for y in 10..101 {
                surface.set(x, y, Surface::LAND);
            }
        }

        let mut teams = vec![TeamStart::new(TeamId(0), Point::new(20, 20))];
        let mut rng = Randomizer::new(0);
        let cargo = populate_map_with_resources(&settings, &surface, &mut teams, &mut rng);

        // Материалы пишутся по чётным (x, y) — как m_point (0,0) в C++.
        let entry = cargo.get(20, 20);

        assert_eq!(entry.resource_type(), Some(ResourceKind::Raw));
        assert!(entry.amount() > 0);

        // Золото — по нечётным x, чётным y (m_point = (1,0)).
        let gold = cargo.get(21, 20);

        assert_eq!(gold.resource_type(), Some(ResourceKind::Gold));
        assert_eq!(gold.visibility(), 0);

        // Топливо — по нечётным (x, y) (m_point = (1,1)).
        let fuel = cargo.get(21, 21);

        assert_eq!(fuel.resource_type(), Some(ResourceKind::Fuel));

        // Чётный x, нечётный y не заполняется нормальным распределением.
        let untouched = cargo.get(20, 21);

        assert_eq!(
            untouched,
            ResourceMapEntry::NONE,
            "parity class (even x, odd y) stays empty"
        );
        assert_eq!(CARGO_GOLD_BITS, 0x40);
        assert_eq!(ResourceMapEntry::from_bits(0).bits(), 0);
        assert_eq!(MAP_CELLS, MAP_TILES);
    }
}
