//! Scanning heat maps, threat maps and zone walking.
//!
//! Oracle: `max-cpp-port/src/heatmap.cpp/.hpp` (`class HeatMap`,
//! `HeatMapCell`, `Add`/`Remove`/`GetComplete`/`GetStealthSea`/
//! `GetStealthLand`/`IsVisible`/`Clear`), `threatmap.cpp/.hpp`
//! (`class ThreatMap`, `GetRiskLevel`, `Update`), `zonewalker.cpp/.hpp`
//! (`class ZoneWalker`), and `access.cpp` (`Access_UpdateMapStatus`,
//! `Access_UpdateMapStatusAddUnit`/`RemoveUnit`, `Access_OnCellRevealed`/
//! `Hidden`/`OnSeaStealthRevealed`/`OnLandStealthRevealed`,
//! `Access_IsVisibleOnHeatMap`, `Access_GetAttackTargetGroup`,
//! `Access_IsWithinScanRange`) and `unitinfo.cpp` (`DrawStealth`).
//!
//! # Model
//!
//! `HeatMap` stores three counter layers per cell (DESIGN §3.4):
//! `complete` (scanning by all units), `stealth_sea` (corvettes) and
//! `stealth_land` (commandos/infantry). Values are `u32` (in the C++ port also
//! `uint32_t`), so there is no overflow on reasonable maps.
//!
//! Unlike C++, where heat maps hang off `TeamInfo` as
//! `std::unique_ptr<HeatMap>`, in max_core they live in [`crate::ai::AiState`]
//! (`heat_maps[team]`): thus they enter the state digest and do not require
//! changing `TeamInfo` (a P3 structure). The semantics are the same: the map exists for
//! every playing team.
//!
//! # Determinism
//!
//! All traversals are by ascending `y`, then `x` (like the C++ `rect_init` loops);
//! traversal of units in a cell — the bucket order of [`crate::hash::MapHash`] (insertion
//! order). RNG is not used.

use crate::access::{self, TARGET_CLASS_AIR, TARGET_CLASS_LAND, TARGET_CLASS_LAND_STEALTH};
use crate::classification::UnitTypeClassify;
use crate::enums::{unit_flags, UnitOrder, UnitType};
use crate::game::GameState;
use crate::geo::{Point, DIRECTION_OFFSETS, MAP_HEIGHT, MAP_TILES, MAP_WIDTH};
use crate::team::{TeamId, TeamKind};
use crate::unit::{UnitId, UnitInfo};

/// `AIPLAYER_THREAT_MAP_CACHE_ENTRIES` (`aiplayer.cpp:62`).
pub const THREAT_MAP_CACHE_ENTRIES: usize = 10;

/// `SURFACE_TYPE_WATER` (see `access.rs`).
const SURFACE_TYPE_WATER: u8 = 0x2;
/// `SURFACE_TYPE_COAST`.
const SURFACE_TYPE_COAST: u8 = 0x4;

// ---------------------------------------------------------------------------
// ZoneWalker
// ---------------------------------------------------------------------------

/// Walk of all cells in a distance square (`class ZoneWalker`).
///
/// A 1:1 port of `zonewalker.cpp`: row-wise traversal, within a row — `x` from
/// `start.x - range` to the symmetric bound limited by the circle.
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

    /// All walk cells in canonical order (for tests and `do/while` loops).
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

/// A heat-map cell — C++ `struct HeatMapCell`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeatMapCell {
    /// Scanning (all units).
    pub complete: u32,
    /// Submarine detection (corvettes).
    pub stealth_sea: u32,
    /// Commando/infantry detection.
    pub stealth_land: u32,
}

/// Team scanning heat map — C++ `class HeatMap`.
///
/// `complete` is always updated; stealth layers — only for units that pass
/// the qualifier (`CORVETTE` → sea, `COMMANDO`/`INFANTRY` → land), as in
/// `resource_manager.cpp:1309-1310`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeatMap {
    /// Owning team (`m_team`).
    pub team: TeamId,
    /// Cells in `y * width + x` order.
    cells: Vec<HeatMapCell>,
}

impl HeatMap {
    /// `HeatMap::HeatMap(width, height, team, …)` — a standard-size map.
    pub fn new(team: TeamId) -> Self {
        Self {
            team,
            cells: vec![HeatMapCell::default(); MAP_TILES],
        }
    }

    /// `HeatMap::GetWidth`/`GetHeight` (standard map).
    pub const fn width(&self) -> i32 {
        MAP_WIDTH as i32
    }

    /// Map height.
    pub const fn height(&self) -> i32 {
        MAP_HEIGHT as i32
    }

    /// `HeatMap::GetCells`.
    pub fn cells(&self) -> &[HeatMapCell] {
        &self.cells
    }

    /// `HeatMap::LoadV70` (`heatmap.cpp:197`) — loading the three layers from a save.
    ///
    /// C++ reads `int8_t[112*112]` and **clamps negative values to 0**
    /// (`legacy < 0 ? 0 : legacy`): in DOS saves uninitialized cells
    /// could be negative. The analog of `complete_clamped` in `save.rs`, but
    /// here — directly into the live `AiState::heat_maps` structure.
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

    /// `HeatMap::GetComplete` (0 off the map).
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

    /// `HeatMap::IsVisible(unit)` — for 2×2 structures four cells are checked.
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

    /// `HeatMap::Add` without callout calls: returns `(complete_was_hidden,
    /// sea_transition, land_transition)`.
    ///
    /// A **stealth transition** is a layer that passed its qualifier and was zero
    /// before this contribution (`heatmap.cpp:67-85`): only a `CORVETTE` raises
    /// `stealth_sea`, only `COMMANDO`/`INFANTRY` raise `stealth_land`, and the
    /// `on_sea_revealed`/`on_land_revealed` callouts fire exactly on those
    /// transitions. A layer that is merely still zero (every cell no corvette
    /// covers) is *not* a transition.
    fn add_raw(&mut self, unit: &UnitInfo, grid_x: i32, grid_y: i32) -> (bool, bool, bool) {
        let Some(index) = Self::index(grid_x, grid_y) else {
            return (false, false, false);
        };

        let cell = &mut self.cells[index];

        let mut sea_transition = false;
        let mut land_transition = false;

        if unit.unit_type == UnitType::Corvette {
            sea_transition = cell.stealth_sea == 0;
            cell.stealth_sea = cell.stealth_sea.wrapping_add(1);
        }

        if matches!(unit.unit_type, UnitType::Infiltrator | UnitType::Infantry) {
            land_transition = cell.stealth_land == 0;
            cell.stealth_land = cell.stealth_land.wrapping_add(1);
        }

        let was_hidden = cell.complete == 0;
        cell.complete = cell.complete.wrapping_add(1);

        (was_hidden, sea_transition, land_transition)
    }

    /// `HeatMap::Remove` without callout calls: returns `complete_was_hidden`.
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
// Integration with GameState: Access_UpdateMapStatus and callouts
// ---------------------------------------------------------------------------

/// Heat-map index of a team or `None` for `ALIEN`.
///
/// In C++ the heat map is a `std::unique_ptr` of the `TeamInfo` slot, and
/// `ResourceManager_InitHeatMaps` creates it only for teams with
/// `team_type != TEAM_TYPE_NONE`; `PLAYER_TEAM_ALIEN` has `team_type` `NONE`, and
/// `heat_map == nullptr`. Therefore all layer accesses for `ALIEN` (and any
/// index outside `0..PLAYER_TEAM_COUNT`) are ignored, like the
/// `if (team_info.heat_map)` checks in `access.cpp`/`unitinfo.cpp`.
#[inline]
fn heat_map_index(team: TeamId) -> Option<usize> {
    let index = team.0 as usize;
    (index < crate::game::PLAYER_TEAM_COUNT).then_some(index)
}

impl GameState {
    /// The temporary heat-map removal idiom: `&mut self.ai.heat_maps` and
    /// unit access via `&mut GameState` are borrow-incompatible.
    ///
    /// `None` — the team has no map (C++ `heat_map == nullptr`), the operation
    /// is skipped.
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
        Self::is_within_scan_range_at(unit.grid, grid_x, grid_y, scan_range)
    }

    /// `Access_IsWithinScanRange` for a **detached** anchor cell.
    ///
    /// The presentation needs the coverage of a unit whose scan anchor is the
    /// cell its *sprite* stands in, not the simulation cell (`App::vision`):
    /// `UnitInfo::Move` (`unitinfo.cpp:1790-1816`) advances `grid_x/grid_y` only
    /// when the sprite crosses into the next cell, so the pure geometry is
    /// shared here instead of being duplicated in the client.
    pub fn is_within_scan_range_at(
        anchor: Point,
        grid_x: i32,
        grid_y: i32,
        scan_range: i32,
    ) -> bool {
        access::is_within_scan_range_at(anchor, grid_x, grid_y, scan_range)
    }

    /// The `ATTRIB_SCAN` radius the unit contributes to `Access_UpdateMapStatus`
    /// (`access.cpp:762-796`), or `None` when the unit does not scan at all.
    ///
    /// This is the single "does this unit scan" rule: [`Self::update_map_status_unit`]
    /// uses it for the simulation, and the client uses it to recompute the same
    /// coverage from the frame's visual anchors (`App::vision`). Keeping one
    /// implementation is what prevents the presentation from drifting from the
    /// simulation.
    pub fn scan_radius(&self, unit: &UnitInfo) -> Option<i32> {
        if unit.orders == UnitOrder::Disable {
            return None;
        }

        if self.teams[unit.team.0 as usize].team_type == TeamKind::None {
            return None;
        }

        // C++ `Access_UpdateMapStatus` updates layers only for teams
        // `PLAYER_TEAM_RED..GRAY`; `PLAYER_TEAM_ALIEN` has no heat map.
        if heat_map_index(unit.team).is_none() {
            return None;
        }

        // `GameManager_AllVisible` reveals every unit through `SpotByTeam`
        // instead of the scan coverage.
        if self.ai.all_visible {
            return None;
        }

        // `unit->GetOrder() != ORDER_IDLE`.
        if unit.orders == UnitOrder::Idle {
            return None;
        }

        if unit.flags & unit_flags::SELECTABLE == 0 {
            return None;
        }

        Some(
            unit.base_values
                .get_attribute(crate::unit_values::ATTRIB_SCAN),
        )
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

    /// `Access_GetAttackTargetGroup` for one unit (`access.cpp:474`).
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

    /// `UnitInfo::DrawStealth(team)` (`unitinfo.cpp:2872`) — updating
    /// unit visibility by scanning and the stealth layers.
    ///
    /// Two outcomes of the oracle:
    /// * the unit is (or becomes) visible — `SpotByTeam`;
    /// * a stealth unit sits in a cell the team has already scanned but the
    ///   matching stealth coverage is gone (`Submarine` without sea-stealth,
    ///   `COMMANDO` without land-stealth) — `Draw(team)`, i.e. the unit is hidden
    ///   again. Without this branch a submarine that lost its corvette coverage
    ///   (the corvette died or drove off while some other scan kept the cell
    ///   covered) stayed visible to every team for the rest of the game.
    fn draw_stealth(world: &mut GameState, id: UnitId, team: TeamId) {
        let Some(unit) = world.unit(id).cloned() else {
            return;
        };

        let underwater = world.is_unit_under_water(&unit);

        if unit.is_detected_by_team(team)
            || (!underwater
                && unit.unit_type != UnitType::Infiltrator
                && unit.unit_type != UnitType::LandMine
                && unit.unit_type != UnitType::SeaMine)
        {
            world.spot_by_team(id, team);
            return;
        }

        // C++ guards both stealth checks with `heat_map != nullptr` (the
        // `ALIEN` slot has no map).
        if heat_map_index(team).is_none() {
            return;
        }

        let grid_x = i32::from(unit.grid.x);
        let grid_y = i32::from(unit.grid.y);

        // `(IsUnitUnderWater && heat_map && !GetStealthSea)` —
        // `(COMMANDO && heat_map && !GetStealthLand)`.
        let lost_cover = (underwater && world.ai_heat_map_stealth_sea(team, grid_x, grid_y) == 0)
            || (unit.unit_type == UnitType::Infiltrator
                && world.ai_heat_map_stealth_land(team, grid_x, grid_y) == 0);

        if lost_cover {
            Self::hide_unit_from_team(world, id, team);
        }
    }

    /// `UnitInfo::Draw(team)` (`unitinfo.cpp:2842`), the simulation part: a unit
    /// that has left the team's scan coverage loses its visibility.
    ///
    /// The C++ sprite bookkeeping (`image_base` of surfaced submarines and
    /// commandos) is presentation; the visibility flag is what the simulation
    /// reads (`UnitInfo::IsVisibleToTeam`).
    fn hide_unit_from_team(world: &mut GameState, id: UnitId, team: TeamId) {
        let Some(unit) = world.unit(id).cloned() else {
            return;
        };

        if unit.team == team || !unit.is_visible_to_team(team) {
            return;
        }

        let mut unit = unit;
        unit.set_visible_to_team(team, false);
        world.write_unit(id, unit);
    }

    /// `Access_DrawUnit` (`access.cpp:691`) вЂ” the per-team visibility pass run by
    /// `Access_UpdateMapStatus(unit, true)` for the unit being updated.
    ///
    /// Heat-map callouts (`Access_OnCellRevealed`) only fire when a cell becomes
    /// covered for the first time. A unit that **moves into an already scanned
    /// cell** therefore has to be spotted by this pass; without it the mover
    /// stays invisible to the scanning team, and its sentry units never react
    /// (`UnitsManager_ShouldAttack` requires `IsVisibleToTeam`).
    fn access_draw_unit(&mut self, unit: &UnitInfo) {
        let grid_x = i32::from(unit.grid.x);
        let grid_y = i32::from(unit.grid.y);

        for team_index in 0..crate::game::PLAYER_TEAM_COUNT {
            let team = TeamId(team_index as u8);

            // C++ `Access_DrawUnit`: `PLAYER_TEAM_RED .. PLAYER_TEAM_MAX - 1`
            // (`PLAYER_TEAM_ALIEN` has no heat map).
            if self.teams[team_index].team_type == TeamKind::None {
                continue;
            }
            if heat_map_index(team).is_none() {
                continue;
            }

            // `heat_map->GetStealthSea(grid)` вЂ” a submerged unit inside the
            // team's sea-stealth coverage is spotted.
            if self.ai_heat_map_stealth_sea(team, grid_x, grid_y) != 0
                && self.is_unit_under_water(unit)
            {
                self.spot_by_team(unit.id, team);
            }

            // `heat_map->GetStealthLand(grid)` вЂ” the same for commandos.
            if unit.unit_type == UnitType::Infiltrator
                && self.ai_heat_map_stealth_land(team, grid_x, grid_y) != 0
            {
                self.spot_by_team(unit.id, team);
            }

            if self.is_visible_on_heat_map(unit, team) {
                Self::draw_stealth(self, unit.id, team);
            } else {
                Self::hide_unit_from_team(self, unit.id, team);
            }
        }
    }

    /// Counting "new targets" when a cell is revealed (`Access_OnCellRevealed`).
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

    /// `Access_OnCellHidden` (in the simulation part — only the mine map).
    fn hide_cell(&mut self, team: TeamId, grid_x: i32, grid_y: i32) {
        self.ai_update_mine_map(team, Point::new(grid_x as i16, grid_y as i16));
    }

    /// `Access_OnSeaStealthRevealed` — detecting underwater units in a cell.
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

    /// `Access_OnLandStealthRevealed` — detecting commandos in a cell.
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

    /// `Survey_SurveyArea` (`survey.cpp:84-121`) — the team's resource knowledge.
    ///
    /// Called from [`GameState::update_map_status_unit`] exactly as C++
    /// (`access.cpp:762-768`: `Access_UpdateMapStatus` → `Survey_SurveyArea`).
    ///
    /// * only for `SURVEYOR`/`MINELAYR`/`SEAMNLYR`/`COMMANDO` and only when
    ///   coverage is added (`mode = true`);
    /// * walking the square `grid ± 1` (radius **1**, i.e. 3×3);
    /// * cells with modified surface `AIR` are skipped;
    /// * **marks** cells only `SURVEYOR` (`survey.cpp:108-117`):
    ///   `ResourceManager_CargoMap` stores the team's `hash_team_id` — in the port
    ///   the [`crate::map::team_visibility_bit`] bit in
    ///   [`crate::map::ResourceMapEntry::visibility`]. Knowledge is **cumulative**
    ///   (`|=`): a scout leaving does not remove it, so `mode = false` rolls back
    ///   nothing — as in C++.
    ///
    /// Mine cells (`Access_GetEnemyMineOnSentry`/`Ai_MarkMineMapPoint`) are
    /// handled separately in the port (`SpotByTeam` on mine detection).
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
    /// Adds the scanning unit's contribution to all three map layers and calls
    /// transition callouts. Returns the bitmask of newly revealed target classes
    /// (`TARGET_CLASS_*`).
    fn update_map_status_add_unit(&mut self, id: UnitId, grid_x: i32, grid_y: i32) -> u32 {
        let Some(unit) = self.unit(id).cloned() else {
            return access::TARGET_CLASS_NONE;
        };

        let team = unit.team;
        if heat_map_index(team).is_none() {
            // C++ `Access_UpdateMapStatusAddUnit`: `heat_map` for `ALIEN` is null.
            return access::TARGET_CLASS_NONE;
        }

        let Some((complete_was_hidden, sea_transition, land_transition)) =
            self.with_heat_map(team, |map, _world| map.add_raw(&unit, grid_x, grid_y))
        else {
            return access::TARGET_CLASS_NONE;
        };

        let mut result = access::TARGET_CLASS_NONE;

        // `Access_UpdateMapStatusAddUnit` (`access.cpp:652-663`): the sea-stealth
        // callout fires only when a **corvette** starts covering the cell
        // (`HeatMap::Add`'s `m_sea_qualifier` + `stealth_sea == 0`). Gating on the
        // raw layer value instead let *any* scanning unit announce the cell and
        // spot every submerged unit in it, so submarines were visible to every
        // team whose ordinary scan touched their cell.
        if sea_transition {
            self.reveal_sea_stealth(team, grid_x, grid_y);

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

        // The same for the land-stealth layer: `COMMANDO`/`INFANTRY` only.
        if land_transition {
            self.reveal_land_stealth(team, grid_x, grid_y);

            let ids: Vec<UnitId> = self
                .units_at(Point::new(grid_x as i16, grid_y as i16))
                .to_vec();
            for other in ids {
                if let Some(other_unit) = self.unit(other) {
                    if other_unit.unit_type == UnitType::Infiltrator && unit.team != other_unit.team
                    {
                        result |= Self::attack_target_group(other_unit);
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
            // C++ `Access_UpdateMapStatusRemoveUnit`: `ALIEN` has no map.
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

    /// `Access_UpdateMapStatus` (`access.cpp:762`) — recomputing the unit's
    /// scanning coverage (`mode = true` — add, `false` — remove).
    ///
    /// In C++ `zone` is the square `grid ± scan`, within which
    /// `Access_IsWithinScanRange` is checked; traversal order — `y`, then `x`.
    pub fn update_map_status(&mut self, id: UnitId, mode: bool) {
        let Some(unit) = self.unit(id).cloned() else {
            return;
        };

        self.update_map_status_unit(&unit, mode);
    }

    /// `Access_UpdateMapStatus` for a unit copy (needed on cell change:
    /// removing coverage at the old position before writing the new one).
    pub fn update_map_status_unit(&mut self, unit: &UnitInfo, mode: bool) {
        let id = unit.id;

        if unit.orders == UnitOrder::Disable {
            return;
        }

        // `Access_UpdateMapStatus` (`access.cpp:762-768`): resource knowledge
        // is updated **before** all coverage checks (`heat_map`, `ORDER_IDLE`,
        // `SELECTABLE`) and only on addition (`mode`). The surveyor marks
        // cells even when in `ORDER_IDLE`.
        if mode {
            self.survey_area(unit);
        }

        let visible_team = unit.team;
        if self.teams[visible_team.0 as usize].team_type == TeamKind::None {
            return;
        }

        // C++ `Access_UpdateMapStatus` updates layers only for teams
        // `PLAYER_TEAM_RED..GRAY`; `PLAYER_TEAM_ALIEN` has no heat map
        // (`ResourceManager_InitHeatMaps`), so alien units do not participate in team
        // heat-map/statistics. The explicit index check is needed
        // because in max_core the `ALIEN` slot is marked `Computer` (see
        // `GameState::with_setup`), unlike `TEAM_TYPE_NONE` in C++.
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

        // `Access_UpdateMapStatus` (`access.cpp:762-816`): for a unit that is not
        // `ORDER_IDLE` the update also refreshes the unit's **own** visibility
        // flags (`Access_DrawUnit`). The scan loop below only records coverage: a
        // unit that moves into a cell the team has already scanned produces no
        // heat-map callout, so without this pass the mover stays invisible to
        // that team вЂ” and its sentry units never receive a reaction
        // (`UnitsManager_ShouldAttack` requires `IsVisibleToTeam`).
        //
        // The removal pass (`mode = false`) is not repeated here: C++ calls it on
        // a `MakeCopy()` of the unit on a cell change (`unitinfo.cpp:1814-1815`),
        // so the `UnitInfo::Draw` there only repaints the temporary copy; the
        // visibility of a moving unit is decided by the following add pass.
        if mode && unit.orders != UnitOrder::Idle {
            self.access_draw_unit(unit);
        }

        // `ORDER_IDLE`, the `SELECTABLE` bit and the `ATTRIB_SCAN` radius: the one
        // rule shared with the presentation (`GameState::scan_radius`).
        let Some(scan) = self.scan_radius(unit) else {
            return;
        };

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

        // `enemy_halt` (`access.cpp:827-868`): an enemy is detected that this
        // unit (or its group) can attack, and the unit has an active
        // order — queue `UnitEventEmergencyStop`, which will be processed in
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

    /// `Access_UpdateVisibilityStatus` (`access.cpp:880`) — a full visibility
    /// recomputation: (optionally) `InitStealthStatus` of all units, clearing
    /// heat maps and re-covering by scanning of each unit.
    ///
    /// Called after army landing — exactly as C++ after
    /// `GameManager_ProcessTeamMissionSupplyUnits` (`game_manager.cpp:1965`):
    /// otherwise units landed earlier "do not see" units landed later (their scan
    /// was computed before the neighbors appeared).
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

        // `Access_InitStealthMaps`: heat maps are always cleared.
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
    /// C++: `heat_map && heat_map->IsVisible(unit)`; `ALIEN` has no map —
    /// `false`.
    pub fn is_visible_on_heat_map(&self, unit: &UnitInfo, team: TeamId) -> bool {
        heat_map_index(team)
            .and_then(|index| self.ai.heat_maps.get(index))
            .is_some_and(|map| map.is_unit_visible(unit))
    }

    /// Cell scanning by a team (`HeatMap::GetComplete`).
    pub fn ai_heat_map_complete(&self, team: TeamId, grid_x: i32, grid_y: i32) -> u32 {
        heat_map_index(team)
            .and_then(|index| self.ai.heat_maps.get(index))
            .map_or(0, |map| map.get_complete(grid_x, grid_y))
    }

    /// Cell sea stealth (`HeatMap::GetStealthSea`).
    pub fn ai_heat_map_stealth_sea(&self, team: TeamId, grid_x: i32, grid_y: i32) -> u32 {
        heat_map_index(team)
            .and_then(|index| self.ai.heat_maps.get(index))
            .map_or(0, |map| map.get_stealth_sea(grid_x, grid_y))
    }

    /// Cell land stealth (`HeatMap::GetStealthLand`).
    pub fn ai_heat_map_stealth_land(&self, team: TeamId, grid_x: i32, grid_y: i32) -> u32 {
        heat_map_index(team)
            .and_then(|index| self.ai.heat_maps.get(index))
            .map_or(0, |map| map.get_stealth_land(grid_x, grid_y))
    }
}

// ---------------------------------------------------------------------------
// ThreatMap
// ---------------------------------------------------------------------------

/// Threat map — C++ `class ThreatMap`.
///
/// Two layers: `damage_potential` (damage an enemy in the cell can deal) and
/// `shots` (the number of shots). Both are `i16` as in C++, so normalization and
/// overflow are reproduced by the same operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreatMap {
    /// Owning team of the map (`team`).
    pub team: i16,
    /// Cache use counter (`id`).
    pub id: u16,
    /// Risk level (`risk_level`, 0 — the map is free).
    pub risk_level: u8,
    /// The caution level the map was built for.
    pub caution_level: u8,
    /// The armor the damage was computed for (`armor`).
    pub armor: i16,
    /// Damage per cell (`damage_potential_map`), index `y * width + x`.
    pub damage_potential_map: Vec<i16>,
    /// Shots per cell (`shots_map`).
    pub shots_map: Vec<i16>,
}

impl Default for ThreatMap {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreatMap {
    /// `ThreatMap::ThreatMap` — an empty map.
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

    /// `ThreatMap::Init` — clearing the maps.
    pub fn init(&mut self) {
        for value in &mut self.damage_potential_map {
            *value = 0;
        }
        for value in &mut self.shots_map {
            *value = 0;
        }
    }

    /// `ThreatMap::Deinit` — release (in Rust — clear + risk reset).
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

    /// `ThreatMap::GetRiskLevel(UnitInfo*)` — lowering risk for
    /// spotted/disarmed units.
    #[allow(dead_code)] // Oracle trace: C++ parity, no gameplay caller yet.
    pub(crate) fn get_risk_level_for_unit(world: &GameState, unit: &UnitInfo) -> i32 {
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

    /// `ThreatMap::Update(armor)` — recomputing damage on an armor change.
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

    /// Damage value in a cell (`damage_potential_map[x][y]`).
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

    /// Shots value in a cell.
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

/// `Access_GetApproximateDistance` (see `access::approximate_distance_points`).
pub fn approximate_distance_points(first: Point, second: Point) -> i32 {
    access::approximate_distance_points(first, second)
}

/// "Around the circle" directions for attack-site search (C++ `DIRECTION_OFFSETS`
/// with step 2 — the four cardinal sides).
pub const CARDINAL_DIRECTIONS: [Point; 4] = [
    DIRECTION_OFFSETS[0],
    DIRECTION_OFFSETS[2],
    DIRECTION_OFFSETS[4],
    DIRECTION_OFFSETS[6],
];

/// `SURFACE_TYPE_WATER | SURFACE_TYPE_COAST`.
pub const SURFACE_WATER_OR_COAST: u8 = SURFACE_TYPE_WATER | SURFACE_TYPE_COAST;

/// Whether the cell is on the map (`in bounds`).
pub fn in_bounds(grid: Point) -> bool {
    grid.x >= 0 && grid.y >= 0 && grid.x < MAP_WIDTH && grid.y < MAP_HEIGHT
}
