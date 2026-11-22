//! Тепловые карты сканирования, карты угроз и обход зоны.
//!
//! Оракул: `max-cpp-port/src/heatmap.cpp/.hpp` (`class HeatMap`,
//! `HeatMapCell`, `Add`/`Remove`/`GetComplete`/`GetStealthSea`/
//! `GetStealthLand`/`IsVisible`/`Clear`), `threatmap.cpp/.hpp`
//! (`class ThreatMap`, `GetRiskLevel`, `Update`), `zonewalker.cpp/.hpp`
//! (`class ZoneWalker`), а также `access.cpp` (`Access_UpdateMapStatus`,
//! `Access_UpdateMapStatusAddUnit`/`RemoveUnit`, `Access_OnCellRevealed`/
//! `Hidden`/`OnSeaStealthRevealed`/`OnLandStealthRevealed`,
//! `Access_IsVisibleOnHeatMap`, `Access_GetAttackTargetGroup`,
//! `Access_IsWithinScanRange`) и `unitinfo.cpp` (`DrawStealth`).
//!
//! # Модель
//!
//! `HeatMap` хранит три слоя счётчиков на клетку (DESIGN §3.4):
//! `complete` (сканирование всех юнитов), `stealth_sea` (корветы) и
//! `stealth_land` (коммандо/пехота). Значения — `u32` (в C++-порте тоже
//! `uint32_t`), поэтому переполнения при разумных картах нет.
//!
//! В отличие от C++, где тепловые карты висят на `TeamInfo` как
//! `std::unique_ptr<HeatMap>`, в max_core они лежат в [`crate::ai::AiState`]
//! (`heat_maps[team]`): так они входят в дайджест состояния и не требуют
//! менять `TeamInfo` (P3-структура). Семантика та же: карта существует для
//! каждой играющей команды.
//!
//! # Детерминизм
//!
//! Все обходы — по возрастанию `y`, затем `x` (как `rect_init`-циклы C++);
//! обход юнитов в клетке — порядок ведра [`crate::hash::MapHash`] (порядок
//! вставки). RNG не используется.

use crate::access::{self, TARGET_CLASS_AIR, TARGET_CLASS_LAND, TARGET_CLASS_LAND_STEALTH};
use crate::classification::UnitTypeClassify;
use crate::enums::{unit_flags, UnitOrder, UnitType};
use crate::game::GameState;
use crate::geo::{Point, DIRECTION_OFFSETS, MAP_HEIGHT, MAP_TILES, MAP_WIDTH};
use crate::team::{TeamId, TeamKind};
use crate::unit::{UnitId, UnitInfo};

/// `AIPLAYER_THREAT_MAP_CACHE_ENTRIES` (`aiplayer.cpp:62`).
pub const THREAT_MAP_CACHE_ENTRIES: usize = 10;

/// `SURFACE_TYPE_WATER` (см. `access.rs`).
const SURFACE_TYPE_WATER: u8 = 0x2;
/// `SURFACE_TYPE_COAST`.
const SURFACE_TYPE_COAST: u8 = 0x4;

// ---------------------------------------------------------------------------
// ZoneWalker
// ---------------------------------------------------------------------------

/// Обход всех клеток в квадрате расстояния (`class ZoneWalker`).
///
/// Порт `zonewalker.cpp` 1:1: обход построчный, внутри строки — `x` от
/// `start.x - range` до симметричной границы, ограниченной кругом.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneWalker {
    start: Point,
    range: i32,
    distance: i32,
    current: Point,
    limit: Point,
}

impl ZoneWalker {
    /// `ZoneWalker::ZoneWalker`.
    pub fn new(position: Point, range: i32) -> Self {
        let mut walker = Self {
            start: position,
            range,
            distance: range * range,
            current: Point::ZERO,
            limit: Point::ZERO,
        };

        walker.limit.y = position.y + range as i16;
        if i32::from(walker.limit.y) > i32::from(MAP_HEIGHT) - 1 {
            walker.limit.y = MAP_HEIGHT - 1;
        }

        walker.init_xy();
        walker
    }

    /// `ZoneWalker::InitXY`.
    fn init_xy(&mut self) {
        self.current.y = self.start.y - self.range as i16;
        if self.current.y < 0 {
            self.current.y = 0;
        }

        self.init_x();
    }

    /// `ZoneWalker::InitX`.
    fn init_x(&mut self) {
        let distance_y =
            i32::from(self.current.y - self.start.y) * i32::from(self.current.y - self.start.y);

        let mut index_x = i32::from(self.start.x) - self.range;
        while index_x <= i32::from(self.start.x) {
            let distance_x =
                (index_x - i32::from(self.start.x)) * (index_x - i32::from(self.start.x));

            if distance_x + distance_y <= self.distance {
                break;
            }

            index_x += 1;
        }

        self.current.x = index_x as i16;
        self.limit.x = (i32::from(self.start.x) * 2 - index_x) as i16;

        if self.current.x < 0 {
            self.current.x = 0;
        }

        if i32::from(self.limit.x) > i32::from(MAP_WIDTH) - 1 {
            self.limit.x = MAP_WIDTH - 1;
        }
    }

    /// `ZoneWalker::GetGridX`.
    pub const fn grid_x(&self) -> i32 {
        self.current.x as i32
    }

    /// `ZoneWalker::GetGridY`.
    pub const fn grid_y(&self) -> i32 {
        self.current.y as i32
    }

    /// `ZoneWalker::GetCurrentLocation`.
    pub const fn current_location(&self) -> Point {
        self.current
    }

    /// `ZoneWalker::FindNext`.
    pub fn find_next(&mut self) -> bool {
        self.current.x += 1;

        if self.current.x <= self.limit.x {
            true
        } else {
            self.current.y += 1;

            if self.current.y <= self.limit.y {
                self.init_x();
                true
            } else {
                false
            }
        }
    }

    /// Все клетки обхода в каноническом порядке (для тестов и циклов `do/while`).
    pub fn cells(position: Point, range: i32) -> Vec<Point> {
        let mut walker = Self::new(position, range);
        let mut cells = Vec::new();

        loop {
            cells.push(walker.current_location());
            if !walker.find_next() {
                break;
            }
        }

        cells
    }
}

// ---------------------------------------------------------------------------
// HeatMap
// ---------------------------------------------------------------------------

/// Клетка тепловой карты — C++ `struct HeatMapCell`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeatMapCell {
    /// Сканирование (все юниты).
    pub complete: u32,
    /// Обнаружение подводных лодок (корветы).
    pub stealth_sea: u32,
    /// Обнаружение коммандо/пехоты.
    pub stealth_land: u32,
}

/// Тепловая карта сканирования команды — C++ `class HeatMap`.
///
/// `complete` всегда обновляется; стелс-слои — только для юнитов, прошедших
/// квалификатор (`CORVETTE` → море, `COMMANDO`/`INFANTRY` → земля), как в
/// `resource_manager.cpp:1309-1310`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeatMap {
    /// Команда-владелец (`m_team`).
    pub team: TeamId,
    /// Клетки в порядке `y * width + x`.
    cells: Vec<HeatMapCell>,
}

impl HeatMap {
    /// `HeatMap::HeatMap(width, height, team, …)` — карта стандартного размера.
    pub fn new(team: TeamId) -> Self {
        Self {
            team,
            cells: vec![HeatMapCell::default(); MAP_TILES],
        }
    }

    /// `HeatMap::GetWidth`/`GetHeight` (стандартная карта).
    pub const fn width(&self) -> i32 {
        MAP_WIDTH as i32
    }

    /// Высота карты.
    pub const fn height(&self) -> i32 {
        MAP_HEIGHT as i32
    }

    /// `HeatMap::GetCells`.
    pub fn cells(&self) -> &[HeatMapCell] {
        &self.cells
    }

    /// `HeatMap::LoadV70` (`heatmap.cpp:197`) — загрузка трёх слоёв из сейва.
    ///
    /// C++ читает `int8_t[112*112]` и **клампит отрицательные значения в 0**
    /// (`legacy < 0 ? 0 : legacy`): в DOS-сейвах неинициализированные клетки
    /// могли быть отрицательными. Аналог `complete_clamped` в `save.rs`, но
    /// здесь — сразу в живую структуру `AiState::heat_maps`.
    pub fn load_v70(
        &mut self,
        complete: &[i8; MAP_TILES],
        stealth_sea: &[i8; MAP_TILES],
        stealth_land: &[i8; MAP_TILES],
    ) {
        let clamp = |value: i8| -> u32 {
            if value < 0 {
                0
            } else {
                value as u32
            }
        };

        for (index, cell) in self.cells.iter_mut().enumerate() {
            *cell = HeatMapCell {
                complete: clamp(complete[index]),
                stealth_sea: clamp(stealth_sea[index]),
                stealth_land: clamp(stealth_land[index]),
            };
        }
    }

    /// `HeatMap::GetIndex` — `y * width + x`.
    fn index(x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= i32::from(MAP_WIDTH) || y >= i32::from(MAP_HEIGHT) {
            return None;
        }

        Some((y as usize) * MAP_WIDTH as usize + x as usize)
    }

    /// `HeatMap::GetComplete` (0 вне карты).
    pub fn get_complete(&self, grid_x: i32, grid_y: i32) -> u32 {
        Self::index(grid_x, grid_y)
            .map(|index| self.cells[index].complete)
            .unwrap_or(0)
    }

    /// `HeatMap::GetStealthSea`.
    pub fn get_stealth_sea(&self, grid_x: i32, grid_y: i32) -> u32 {
        Self::index(grid_x, grid_y)
            .map(|index| self.cells[index].stealth_sea)
            .unwrap_or(0)
    }

    /// `HeatMap::GetStealthLand`.
    pub fn get_stealth_land(&self, grid_x: i32, grid_y: i32) -> u32 {
        Self::index(grid_x, grid_y)
            .map(|index| self.cells[index].stealth_land)
            .unwrap_or(0)
    }

    /// `HeatMap::IsVisible(x, y)`.
    pub fn is_visible(&self, grid_x: i32, grid_y: i32) -> bool {
        self.get_complete(grid_x, grid_y) > 0
    }

    /// `HeatMap::IsVisible(unit)` — у 2×2-структур проверяются четыре клетки.
    pub fn is_unit_visible(&self, unit: &UnitInfo) -> bool {
        if unit.is_quad() {
            return self.get_complete(i32::from(unit.grid.x), i32::from(unit.grid.y)) > 0
                || self.get_complete(i32::from(unit.grid.x) + 1, i32::from(unit.grid.y)) > 0
                || self.get_complete(i32::from(unit.grid.x), i32::from(unit.grid.y) + 1) > 0
                || self.get_complete(i32::from(unit.grid.x) + 1, i32::from(unit.grid.y) + 1) > 0;
        }

        self.get_complete(i32::from(unit.grid.x), i32::from(unit.grid.y)) > 0
    }

    /// `HeatMap::Clear`.
    pub fn clear(&mut self) {
        for cell in &mut self.cells {
            *cell = HeatMapCell::default();
        }
    }

    /// `HeatMap::Add` без вызовов callout: возвращает `(complete_was_hidden,
    /// sea_was_hidden, land_was_hidden)`.
    fn add_raw(&mut self, unit: &UnitInfo, grid_x: i32, grid_y: i32) -> (bool, bool, bool) {
        let Some(index) = Self::index(grid_x, grid_y) else {
            return (false, false, false);
        };

        let cell = &mut self.cells[index];
        let sea_was_zero = cell.stealth_sea == 0;
        let land_was_zero = cell.stealth_land == 0;

        if unit.unit_type == UnitType::Corvette {
            cell.stealth_sea = cell.stealth_sea.wrapping_add(1);
        }

        if matches!(unit.unit_type, UnitType::Infiltrator | UnitType::Infantry) {
            cell.stealth_land = cell.stealth_land.wrapping_add(1);
        }

        let was_hidden = cell.complete == 0;
        cell.complete = cell.complete.wrapping_add(1);

        (was_hidden, sea_was_zero, land_was_zero)
    }

    /// `HeatMap::Remove` без вызовов callout: возвращает `complete_was_hidden`.
    fn remove_raw(&mut self, unit: &UnitInfo, grid_x: i32, grid_y: i32) -> bool {
        let Some(index) = Self::index(grid_x, grid_y) else {
            return false;
        };

        let cell = &mut self.cells[index];

        if unit.unit_type == UnitType::Corvette && cell.stealth_sea > 0 {
            cell.stealth_sea -= 1;
        }

        if matches!(unit.unit_type, UnitType::Infiltrator | UnitType::Infantry)
            && cell.stealth_land > 0
        {
            cell.stealth_land -= 1;
        }

        if cell.complete > 0 {
            cell.complete -= 1;
        }

        cell.complete == 0
    }
}

// ---------------------------------------------------------------------------
// Интеграция с GameState: Access_UpdateMapStatus и callouts
// ---------------------------------------------------------------------------

/// Индекс тепловой карты команды или `None` для `ALIEN`.
///
/// В C++ тепловая карта — `std::unique_ptr` слота `TeamInfo`, и
/// `ResourceManager_InitHeatMaps` создаёт её только для команд с
/// `team_type != TEAM_TYPE_NONE`; у `PLAYER_TEAM_ALIEN` `team_type` — `NONE`, а
/// `heat_map == nullptr`. Поэтому все обращения к слоям для `ALIEN` (и любого
/// индекса вне `0..PLAYER_TEAM_COUNT`) игнорируются, как проверки
/// `if (team_info.heat_map)` в `access.cpp`/`unitinfo.cpp`.
#[inline]
fn heat_map_index(team: TeamId) -> Option<usize> {
    let index = team.0 as usize;
    (index < crate::game::PLAYER_TEAM_COUNT).then_some(index)
}

impl GameState {
    /// Идиома временного изъятия тепловой карты: `&mut self.ai.heat_maps` и
    /// доступ к юнитам через `&mut GameState` несовместимы по заимствованию.
    ///
    /// `None` — у команды нет карты (C++ `heat_map == nullptr`), операция
    /// пропускается.
    fn with_heat_map<R>(
        &mut self,
        team: TeamId,
        f: impl FnOnce(&mut HeatMap, &mut GameState) -> R,
    ) -> Option<R> {
        let index = heat_map_index(team)?;
        let mut map = std::mem::replace(&mut self.ai.heat_maps[index], HeatMap::new(team));
        let result = f(&mut map, self);
        self.ai.heat_maps[index] = map;
        Some(result)
    }

    /// `Access_IsWithinScanRange` (`access.cpp:353`).
    pub fn is_within_scan_range(
        unit: &UnitInfo,
        grid_x: i32,
        grid_y: i32,
        scan_range: i32,
    ) -> bool {
        let scan_area = (scan_range * 64) * (scan_range * 64);
        let radius_x = (grid_x - i32::from(unit.grid.x)) * 64;
        let radius_y = (grid_y - i32::from(unit.grid.y)) * 64;

        radius_x * radius_x + radius_y * radius_y <= scan_area
    }

    /// `UnitsManager_IsUnitUnderWater` (`units_manager.cpp`).
    pub fn is_unit_under_water(&self, unit: &UnitInfo) -> bool {
        if unit.unit_type == UnitType::Submarine {
            return true;
        }

        if unit.unit_type == UnitType::ArmouredPersonnelCarrier {
            return access::modified_surface_type(self, unit.grid, false)
                == access::SURFACE_TYPE_WATER;
        }

        false
    }

    /// `Access_GetAttackTargetGroup` для одного юнита (`access.cpp:474`).
    pub fn attack_target_group(unit: &UnitInfo) -> u32 {
        if unit
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_ATTACK)
            == 0
            || unit.orders == UnitOrder::Disable
        {
            return access::TARGET_CLASS_NONE;
        }

        use crate::access::{TARGET_CLASS_NONE, TARGET_CLASS_SEA_STEALTH, TARGET_CLASS_WATER};

        match unit.unit_type {
            UnitType::Infiltrator | UnitType::Infantry => {
                TARGET_CLASS_LAND_STEALTH | TARGET_CLASS_LAND | TARGET_CLASS_WATER
            }
            UnitType::Corvette => TARGET_CLASS_SEA_STEALTH | TARGET_CLASS_WATER,
            UnitType::GunTurret
            | UnitType::Artillery
            | UnitType::MissileLauncher
            | UnitType::Scout
            | UnitType::Tank
            | UnitType::AssaultGun
            | UnitType::RocketLauncher
            | UnitType::MissileCrawler
            | UnitType::Escort
            | UnitType::Gunboat
            | UnitType::MissileCruiser
            | UnitType::Bomber
            | UnitType::Juggernaut
            | UnitType::AlienTank
            | UnitType::AlienAssaultGun => TARGET_CLASS_LAND | TARGET_CLASS_WATER,
            UnitType::Submarine => TARGET_CLASS_WATER,
            UnitType::AntiAir | UnitType::MobileAntiAir | UnitType::Fighter => TARGET_CLASS_AIR,
            UnitType::AlienPlane => TARGET_CLASS_LAND | TARGET_CLASS_WATER | TARGET_CLASS_AIR,
            UnitType::LandMine | UnitType::SeaMine => TARGET_CLASS_NONE,
            _ => TARGET_CLASS_NONE,
        }
    }

    /// `UnitInfo::DrawStealth(team)` (`unitinfo.cpp:2872`) — обновление
    /// видимости юнита по сканированию и стелс-слоям.
    fn draw_stealth(world: &mut GameState, id: UnitId, team: TeamId) {
        let Some(unit) = world.unit(id).cloned() else {
            return;
        };

        if unit.is_detected_by_team(team)
            || (!world.is_unit_under_water(&unit)
                && unit.unit_type != UnitType::Infiltrator
                && unit.unit_type != UnitType::LandMine
                && unit.unit_type != UnitType::SeaMine)
        {
            world.spot_by_team(id, team);
        }
    }

    /// Подсчёт «новых целей» при открытии клетки (`Access_OnCellRevealed`).
    fn reveal_cell(&mut self, team: TeamId, grid_x: i32, grid_y: i32) -> u32 {
        let grid = Point::new(grid_x as i16, grid_y as i16);

        self.ai_set_info_map_point(team, grid);

        let ids: Vec<UnitId> = self.units_at(grid).to_vec();
        let mut revealed_target_class = access::TARGET_CLASS_NONE;

        for id in ids {
            let Some(unit) = self.unit(id) else { continue };
            if unit.is_visible_to_team(team) {
                continue;
            }

            Self::draw_stealth(self, id, team);

            if let Some(unit) = self.unit(id) {
                if unit.is_visible_to_team(team) && unit.team != team {
                    revealed_target_class |= Self::attack_target_group(unit);
                }
            }
        }

        revealed_target_class
    }

    /// `Access_OnCellHidden` (в симуляционной части — только карта мин).
    fn hide_cell(&mut self, team: TeamId, grid_x: i32, grid_y: i32) {
        self.ai_update_mine_map(team, Point::new(grid_x as i16, grid_y as i16));
    }

    /// `Access_OnSeaStealthRevealed` — обнаружение подводных юнитов в клетке.
    fn reveal_sea_stealth(&mut self, team: TeamId, grid_x: i32, grid_y: i32) {
        let ids: Vec<UnitId> = self
            .units_at(Point::new(grid_x as i16, grid_y as i16))
            .to_vec();

        for id in ids {
            let under_water = self
                .unit(id)
                .map(|unit| self.is_unit_under_water(unit))
                .unwrap_or(false);

            if under_water {
                self.spot_by_team(id, team);
            }
        }
    }

    /// `Access_OnLandStealthRevealed` — обнаружение коммандо в клетке.
    fn reveal_land_stealth(&mut self, team: TeamId, grid_x: i32, grid_y: i32) {
        let ids: Vec<UnitId> = self
            .units_at(Point::new(grid_x as i16, grid_y as i16))
            .to_vec();

        for id in ids {
            let is_commando = self
                .unit(id)
                .map(|unit| unit.unit_type == UnitType::Infiltrator)
                .unwrap_or(false);

            if is_commando {
                self.spot_by_team(id, team);
            }
        }
    }

    /// `Survey_SurveyArea` (`survey.cpp:84-121`) — знание ресурсов команды.
    ///
    /// Вызывается из [`GameState::update_map_status_unit`] ровно как C++
    /// (`access.cpp:762-768`: `Access_UpdateMapStatus` → `Survey_SurveyArea`).
    ///
    /// * только для `SURVEYOR`/`MINELAYR`/`SEAMNLYR`/`COMMANDO` и только при
    ///   добавлении покрытия (`mode = true`);
    /// * обход квадрата `grid ± 1` (радиус **1**, т.е. 3×3);
    /// * клетки с модифицированной поверхностью `AIR` пропускаются;
    /// * **помечает** клетки только `SURVEYOR` (`survey.cpp:108-117`): в
    ///   `ResourceManager_CargoMap` ставится `hash_team_id` команды — у порта
    ///   бит [`crate::map::team_visibility_bit`] в
    ///   [`crate::map::ResourceMapEntry::visibility`]. Знание **накопительное**
    ///   (`|=`): уход разведчика его не снимает, поэтому `mode = false` ничего
    ///   не откатывает — как в C++.
    ///
    /// Клетки мин (`Access_GetEnemyMineOnSentry`/`Ai_MarkMineMapPoint`) в
    /// порте обрабатываются отдельно (`SpotByTeam` при обнаружении мины).
    pub fn survey_area(&mut self, unit: &UnitInfo) {
        if !matches!(
            unit.unit_type,
            UnitType::Surveyor
                | UnitType::MineLayer
                | UnitType::SeaMineLayer
                | UnitType::Infiltrator
        ) {
            return;
        }

        let radius = 1;
        let grid_x = i32::from(unit.grid.x);
        let grid_y = i32::from(unit.grid.y);
        let ulx = (grid_x - radius).max(0);
        let uly = (grid_y - radius).max(0);
        let lrx = (grid_x + radius).min(i32::from(MAP_WIDTH) - 1);
        let lry = (grid_y + radius).min(i32::from(MAP_HEIGHT) - 1);

        for y in uly..=lry {
            for x in ulx..=lrx {
                let point = Point::new(x as i16, y as i16);

                if access::modified_surface_type(self, point, false) == access::SURFACE_TYPE_AIR {
                    continue;
                }

                if unit.unit_type == UnitType::Surveyor {
                    let entry = self.map.resources.get(point.x, point.y);
                    let known = entry.with_team_visible(unit.team.0);

                    if known != entry {
                        self.map.resources.set(point.x, point.y, known);
                    }
                }
            }
        }
    }

    /// `Access_UpdateMapStatusAddUnit` (`access.cpp:637`).
    ///
    /// Добавляет вклад сканирующего юнита во все три слоя карты и вызывает
    /// callout'ы переходов. Возвращает битовую маску впервые открытых классов
    /// целей (`TARGET_CLASS_*`).
    fn update_map_status_add_unit(&mut self, id: UnitId, grid_x: i32, grid_y: i32) -> u32 {
        let Some(unit) = self.unit(id).cloned() else {
            return access::TARGET_CLASS_NONE;
        };

        let team = unit.team;
        if heat_map_index(team).is_none() {
            // C++ `Access_UpdateMapStatusAddUnit`: `heat_map` у `ALIEN` — null.
            return access::TARGET_CLASS_NONE;
        }

        let sea_was_zero = self.ai_heat_map_stealth_sea(team, grid_x, grid_y) == 0;
        let land_was_zero = self.ai_heat_map_stealth_land(team, grid_x, grid_y) == 0;

        let Some((complete_was_hidden, sea_transition, land_transition)) =
            self.with_heat_map(team, |map, _world| map.add_raw(&unit, grid_x, grid_y))
        else {
            return access::TARGET_CLASS_NONE;
        };

        let mut result = access::TARGET_CLASS_NONE;

        if sea_transition || (unit.unit_type == UnitType::Corvette && sea_was_zero) {
            self.reveal_sea_stealth(team, grid_x, grid_y);

            if unit.unit_type == UnitType::Corvette {
                let ids: Vec<UnitId> = self
                    .units_at(Point::new(grid_x as i16, grid_y as i16))
                    .to_vec();
                for other in ids {
                    if let Some(other_unit) = self.unit(other) {
                        if self.is_unit_under_water(other_unit) && unit.team != other_unit.team {
                            result |= Self::attack_target_group(other_unit);
                        }
                    }
                }
            }
        }

        if land_transition
            || (matches!(unit.unit_type, UnitType::Infiltrator | UnitType::Infantry)
                && land_was_zero)
        {
            self.reveal_land_stealth(team, grid_x, grid_y);

            if matches!(unit.unit_type, UnitType::Infiltrator | UnitType::Infantry) {
                let ids: Vec<UnitId> = self
                    .units_at(Point::new(grid_x as i16, grid_y as i16))
                    .to_vec();
                for other in ids {
                    if let Some(other_unit) = self.unit(other) {
                        if other_unit.unit_type == UnitType::Infiltrator
                            && unit.team != other_unit.team
                        {
                            result |= Self::attack_target_group(other_unit);
                        }
                    }
                }
            }
        }

        if complete_was_hidden {
            result |= self.reveal_cell(team, grid_x, grid_y);
            let _ = complete_was_hidden;
        }

        result
    }

    /// `Access_UpdateMapStatusRemoveUnit` (`access.cpp:684`).
    fn update_map_status_remove_unit(&mut self, id: UnitId, grid_x: i32, grid_y: i32) {
        let Some(unit) = self.unit(id).cloned() else {
            return;
        };

        let team = unit.team;
        if heat_map_index(team).is_none() {
            // C++ `Access_UpdateMapStatusRemoveUnit`: у `ALIEN` карты нет.
            return;
        }

        let Some(became_hidden) =
            self.with_heat_map(team, |map, _world| map.remove_raw(&unit, grid_x, grid_y))
        else {
            return;
        };

        if became_hidden {
            self.hide_cell(team, grid_x, grid_y);
        }
    }

    /// `Access_UpdateMapStatus` (`access.cpp:762`) — пересчёт покрытия
    /// сканирования юнита (`mode = true` — добавление, `false` — снятие).
    ///
    /// В C++ `zone` — квадрат `grid ± scan`, внутри которого проверяется
    /// `Access_IsWithinScanRange`; порядок обхода — `y`, затем `x`.
    pub fn update_map_status(&mut self, id: UnitId, mode: bool) {
        let Some(unit) = self.unit(id).cloned() else {
            return;
        };

        self.update_map_status_unit(&unit, mode);
    }

    /// `Access_UpdateMapStatus` для копии юнита (нужна при смене клетки:
    /// снятие покрытия по старой позиции до записи новой).
    pub fn update_map_status_unit(&mut self, unit: &UnitInfo, mode: bool) {
        let id = unit.id;

        if unit.orders == UnitOrder::Disable {
            return;
        }

        // `Access_UpdateMapStatus` (`access.cpp:762-768`): знание ресурсов
        // обновляется **до** всех проверок покрытия (`heat_map`, `ORDER_IDLE`,
        // `SELECTABLE`) и только на добавлении (`mode`). Разведчик помечает
        // клетки, даже когда стоит `ORDER_IDLE`.
        if mode {
            self.survey_area(unit);
        }

        let visible_team = unit.team;
        if self.teams[visible_team.0 as usize].team_type == TeamKind::None {
            return;
        }

        // C++ `Access_UpdateMapStatus` обновляет слои только для команд
        // `PLAYER_TEAM_RED..GRAY`; у `PLAYER_TEAM_ALIEN` тепловой карты нет
        // (`ResourceManager_InitHeatMaps`), поэтому alien-юниты в командных
        // heat-map/статистиках не участвуют. Явная проверка индекса нужна
        // потому, что в max_core слот `ALIEN` помечен `Computer` (см.
        // `GameState::with_setup`), в отличие от `TEAM_TYPE_NONE` в C++.
        if heat_map_index(visible_team).is_none() {
            return;
        }

        if self.ai.all_visible {
            if mode {
                for team in 0..crate::game::PLAYER_TEAM_COUNT {
                    self.spot_by_team(id, TeamId(team as u8));
                }
            }
            return;
        }

        if unit.orders == UnitOrder::Idle {
            return;
        }

        if unit.flags & unit_flags::SELECTABLE == 0 {
            return;
        }

        let mut scan = unit
            .base_values
            .get_attribute(crate::unit_values::ATTRIB_SCAN);
        if unit.orders == UnitOrder::Disable {
            scan = 0;
        }

        let mut ulx = i32::from(unit.grid.x) - scan;
        let mut uly = i32::from(unit.grid.y) - scan;
        let mut lrx = i32::from(unit.grid.x) + scan;
        let mut lry = i32::from(unit.grid.y) + scan;

        if ulx < 0 {
            ulx = 0;
        }
        if uly < 0 {
            uly = 0;
        }
        if lrx > i32::from(MAP_WIDTH) - 1 {
            lrx = i32::from(MAP_WIDTH) - 1;
        }
        if lry > i32::from(MAP_HEIGHT) - 1 {
            lry = i32::from(MAP_HEIGHT) - 1;
        }

        let mut grid_y = uly;
        let mut enemy_target_class = access::TARGET_CLASS_NONE;

        while grid_y <= lry {
            let mut grid_x = ulx;
            while grid_x <= lrx {
                if Self::is_within_scan_range(unit, grid_x, grid_y, scan) {
                    if mode {
                        enemy_target_class |= self.update_map_status_add_unit(id, grid_x, grid_y);
                    } else {
                        self.update_map_status_remove_unit(id, grid_x, grid_y);
                    }
                }
                grid_x += 1;
            }
            grid_y += 1;
        }

        // `enemy_halt` (`access.cpp:827-868`): обнаружен враг, которого этот
        // юнит (или его группа) может атаковать, а у юнита есть активный
        // приказ — ставим `UnitEventEmergencyStop`, который обработается в
        // `UnitsManager_ProcessOrders` (`GameState::process_orders`).
        if !mode || enemy_target_class == access::TARGET_CLASS_NONE || !self.enemy_halt {
            return;
        }

        let team_type = self.teams[visible_team.0 as usize].team_type;
        if !matches!(team_type, TeamKind::Player | TeamKind::Computer) {
            return;
        }

        // `unit->GetOrder() != ORDER_AWAIT`.
        if unit.orders == UnitOrder::Await {
            return;
        }

        let mut friendly_target_class = access::target_class(self, unit);

        if let Some(members) = self.group_members(id) {
            for member in members {
                if let Some(member_unit) = self.unit(member) {
                    friendly_target_class |= access::target_class(self, member_unit);
                }
            }
        }

        if friendly_target_class & enemy_target_class == 0 {
            return;
        }

        let stops: Vec<UnitId> = match self.group_members(id) {
            Some(members) => members,
            None => vec![id],
        };

        for stopped in stops {
            self.unit_events
                .push(crate::events::UnitEvent::EmergencyStop { unit: stopped });
        }
    }

    /// `Access_UpdateVisibilityStatus` (`access.cpp:880`) — полный пересчёт
    /// видимости: (опционально) `InitStealthStatus` всех юнитов, очистка
    /// тепловых карт и повторное покрытие сканированием каждого юнита.
    ///
    /// Вызывается после высадки армий — ровно как C++ после
    /// `GameManager_ProcessTeamMissionSupplyUnits` (`game_manager.cpp:1965`):
    /// иначе юниты, высаженные раньше, «не видят» высаженных позже (их скан
    /// считался до появления соседей).
    pub fn update_visibility_status(&mut self, reset_visibility: bool) {
        let ids: Vec<UnitId> = self.unit_ids_in_order();
        let all_visible = self.ai.all_visible;

        if reset_visibility {
            for id in &ids {
                if let Some(mut unit) = self.unit(*id).cloned() {
                    crate::combat::init_stealth_status(&mut unit, all_visible);
                    self.write_unit(*id, unit);
                }
            }
        }

        // `Access_InitStealthMaps`: тепловые карты очищаются всегда.
        for map in &mut self.ai.heat_maps {
            map.clear();
        }

        if !all_visible {
            for id in ids {
                self.update_map_status(id, true);
            }
        }
    }

    /// `Access_IsVisibleOnHeatMap` (`access.cpp:538`).
    ///
    /// C++: `heat_map && heat_map->IsVisible(unit)`; у `ALIEN` карты нет —
    /// `false`.
    pub fn is_visible_on_heat_map(&self, unit: &UnitInfo, team: TeamId) -> bool {
        heat_map_index(team)
            .and_then(|index| self.ai.heat_maps.get(index))
            .is_some_and(|map| map.is_unit_visible(unit))
    }

    /// Сканирование клетки командой (`HeatMap::GetComplete`).
    pub fn ai_heat_map_complete(&self, team: TeamId, grid_x: i32, grid_y: i32) -> u32 {
        heat_map_index(team)
            .and_then(|index| self.ai.heat_maps.get(index))
            .map_or(0, |map| map.get_complete(grid_x, grid_y))
    }

    /// Стелс-море клетки (`HeatMap::GetStealthSea`).
    pub fn ai_heat_map_stealth_sea(&self, team: TeamId, grid_x: i32, grid_y: i32) -> u32 {
        heat_map_index(team)
            .and_then(|index| self.ai.heat_maps.get(index))
            .map_or(0, |map| map.get_stealth_sea(grid_x, grid_y))
    }

    /// Стелс-земля клетки (`HeatMap::GetStealthLand`).
    pub fn ai_heat_map_stealth_land(&self, team: TeamId, grid_x: i32, grid_y: i32) -> u32 {
        heat_map_index(team)
            .and_then(|index| self.ai.heat_maps.get(index))
            .map_or(0, |map| map.get_stealth_land(grid_x, grid_y))
    }
}

// ---------------------------------------------------------------------------
// ThreatMap
// ---------------------------------------------------------------------------

/// Карта угроз — C++ `class ThreatMap`.
///
/// Два слоя: `damage_potential` (урон, который может нанести враг в клетке) и
/// `shots` (число выстрелов). Оба — `i16` как в C++, поэтому нормализация и
/// переполнение воспроизводятся теми же операциями.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreatMap {
    /// Команда-владелец карты (`team`).
    pub team: i16,
    /// Счётчик использования в кэше (`id`).
    pub id: u16,
    /// Уровень риска (`risk_level`, 0 — карта свободна).
    pub risk_level: u8,
    /// Уровень осторожности, для которого построена карта.
    pub caution_level: u8,
    /// Броня, для которой посчитан урон (`armor`).
    pub armor: i16,
    /// Урон на клетку (`damage_potential_map`), индекс `y * width + x`.
    pub damage_potential_map: Vec<i16>,
    /// Выстрелы на клетку (`shots_map`).
    pub shots_map: Vec<i16>,
}

impl Default for ThreatMap {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreatMap {
    /// `ThreatMap::ThreatMap` — пустая карта.
    pub fn new() -> Self {
        Self {
            team: 0,
            id: 0,
            risk_level: 0,
            caution_level: 0,
            armor: 0,
            damage_potential_map: vec![0; MAP_TILES],
            shots_map: vec![0; MAP_TILES],
        }
    }

    /// `ThreatMap::Init` — очистка карт.
    pub fn init(&mut self) {
        for value in &mut self.damage_potential_map {
            *value = 0;
        }
        for value in &mut self.shots_map {
            *value = 0;
        }
    }

    /// `ThreatMap::Deinit` — освобождение (в Rust — очистка + сброс риска).
    pub fn deinit(&mut self) {
        self.risk_level = 0;
        self.init();
    }

    /// `ThreatMap::SetRiskLevel`.
    pub fn set_risk_level(&mut self, risk_level: u8) {
        self.risk_level = risk_level;
    }

    /// `ThreatMap::GetRiskLevel(ResourceID)` (`threatmap.cpp:96`).
    pub fn get_risk_level_for_type(unit_type: UnitType) -> i32 {
        match unit_type {
            UnitType::ArmouredPersonnelCarrier => 7,
            UnitType::Submarine => 6,
            UnitType::Infiltrator => 5,
            UnitType::Surveyor => 2,
            _ => {
                if unit_type.is_air() {
                    3
                } else {
                    1
                }
            }
        }
    }

    /// `ThreatMap::GetRiskLevel(UnitInfo*)` — понижение риска для
    /// обнаруженных/разряженных юнитов.
    pub fn get_risk_level_for_unit(world: &GameState, unit: &UnitInfo) -> i32 {
        let mut result = Self::get_risk_level_for_type(unit.unit_type);

        if result >= 4 {
            let mut team_index = 0;
            while team_index < crate::game::PLAYER_TEAM_COUNT {
                let team = TeamId(team_index as u8);
                if unit.team != team && unit.is_detected_by_team(team) {
                    break;
                }
                team_index += 1;
            }

            if team_index < crate::game::PLAYER_TEAM_COUNT {
                result = 1;
            } else if result == 5 && unit.shots == 0 {
                result = 4;
            }
        }

        let _ = world;
        result
    }

    /// `ThreatMap::Update(armor)` — пересчёт урона при смене брони.
    pub fn update(&mut self, armor: i32) {
        if i32::from(self.armor) != armor {
            let difference = i32::from(self.armor) - armor;
            self.armor = armor as i16;

            for index in 0..self.damage_potential_map.len() {
                let shots = i32::from(self.shots_map[index]);
                let current = i32::from(self.damage_potential_map[index]);
                self.damage_potential_map[index] = (current + shots * difference) as i16;
            }
        }
    }

    /// Значение урона в клетке (`damage_potential_map[x][y]`).
    pub fn damage_at(&self, grid_x: i32, grid_y: i32) -> i32 {
        if grid_x < 0
            || grid_y < 0
            || grid_x >= i32::from(MAP_WIDTH)
            || grid_y >= i32::from(MAP_HEIGHT)
        {
            return 0;
        }

        i32::from(
            self.damage_potential_map[(grid_y as usize) * MAP_WIDTH as usize + grid_x as usize],
        )
    }

    /// Значение выстрелов в клетке.
    pub fn shots_at(&self, grid_x: i32, grid_y: i32) -> i32 {
        if grid_x < 0
            || grid_y < 0
            || grid_x >= i32::from(MAP_WIDTH)
            || grid_y >= i32::from(MAP_HEIGHT)
        {
            return 0;
        }

        i32::from(self.shots_map[(grid_y as usize) * MAP_WIDTH as usize + grid_x as usize])
    }
}

/// `Access_GetApproximateDistance` (см. `access::approximate_distance_points`).
pub fn approximate_distance_points(first: Point, second: Point) -> i32 {
    access::approximate_distance_points(first, second)
}

/// Направления «по кругу» для поиска сайтов атаки (C++ `DIRECTION_OFFSETS`
/// с шагом 2 — 4 стороны света).
pub const CARDINAL_DIRECTIONS: [Point; 4] = [
    DIRECTION_OFFSETS[0],
    DIRECTION_OFFSETS[2],
    DIRECTION_OFFSETS[4],
    DIRECTION_OFFSETS[6],
];

/// `SURFACE_TYPE_WATER | SURFACE_TYPE_COAST`.
pub const SURFACE_WATER_OR_COAST: u8 = SURFACE_TYPE_WATER | SURFACE_TYPE_COAST;

/// На карте ли клетка (`in bounds`).
pub fn in_bounds(grid: Point) -> bool {
    grid.x >= 0 && grid.y >= 0 && grid.x < MAP_WIDTH && grid.y < MAP_HEIGHT
}
