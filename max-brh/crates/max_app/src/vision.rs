//! Frame-side vision (scan) that follows the **visible** sprite.
//!
//! Oracle: `Access_UpdateMapStatus` (`max-cpp-port/src/access.cpp:762`) is called
//! from `UnitInfo::Move` exactly when the sprite crosses into the next cell
//! (`unitinfo.cpp:1787-1816`: `grid_x = x / GFX_MAP_TILE_SIZE` and then
//! `Access_UpdateMapStatus(this, true)` / `(old copy, false)`), so in the original
//! the radar of a unit moves together with its sprite: the vision is updated on
//! **arrival** at the cell, never earlier.
//!
//! The port's lockstep simulation advances a whole cell per tick while the frame
//! walks the same cells at [`crate::VISUAL_CELLS_PER_SECOND`] (see
//! [`crate::App::update_visual_units`]), so reading the simulation scan on the
//! frame shows the radar ahead of the unit — the manual defect «радар (scan)
//! смещается раньше прибытия юнита».
//!
//! This module therefore keeps a frame-only copy of the player's **current**
//! coverage:
//!
//! * [`VisionView::covered`] — the cells covered by the own units' scan at their
//!   **visual** anchor (the cell the sprite stands in). This is the only vision
//!   state: the original keeps no explored memory. `HeatMap::complete` is
//!   incremented/decremented live (`heatmap.cpp:56-141`), `Access_OnCellHidden`
//!   (`access.cpp:576`) darkens `MinimapFov` and clears `visible_to_team` of the
//!   units in the cell, and `Access_UpdateMinimapFogOfWar` (`access.cpp:896`)
//!   darkens every cell with `complete == 0`. A scanner that leaves therefore
//!   closes the fog and hides the enemy again — «туман закрывается».
//!
//! It never writes to the simulation, is not part of `snapshot_bytes`/CRC and is
//! rebuilt from the simulation for `dt <= 0` (QA/headless frames), so the golden
//! frames stay byte-identical.

use std::collections::BTreeMap;

use max_core::enums::unit_flags;
use max_core::geo::{Point, MAP_TILES};
use max_core::unit::{UnitId, UnitInfo};
use max_core::universe::StateRef;

/// Number of `u32` words of the [`CellMask`] bit set.
const MASK_WORDS: usize = (MAP_TILES + 31) / 32;

/// A bit set over the map cells (`MAP_WIDTH * MAP_HEIGHT`, `y * width + x`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellMask {
    words: Vec<u32>,
}

impl Default for CellMask {
    fn default() -> Self {
        Self::new()
    }
}

impl CellMask {
    /// An empty mask.
    pub fn new() -> Self {
        Self {
            words: vec![0; MASK_WORDS],
        }
    }

    /// Clears every cell.
    pub fn clear(&mut self) {
        for word in &mut self.words {
            *word = 0;
        }
    }

    /// Sets the cell with the flat index (`y * MAP_WIDTH + x`).
    pub fn set_index(&mut self, index: usize) {
        if index < MAP_TILES {
            self.words[index / 32] |= 1 << (index % 32);
        }
    }

    /// Whether the cell with the flat index is set.
    pub fn get_index(&self, index: usize) -> bool {
        index < MAP_TILES && self.words[index / 32] & (1 << (index % 32)) != 0
    }

    /// Sets a map cell; out-of-map points are ignored.
    pub fn set(&mut self, x: i32, y: i32) {
        if x >= 0
            && y >= 0
            && x < i32::from(max_core::geo::MAP_WIDTH)
            && y < i32::from(max_core::geo::MAP_HEIGHT)
        {
            self.set_index(y as usize * max_core::geo::MAP_WIDTH as usize + x as usize);
        }
    }

    /// Whether a map cell is set; out-of-map points are never set.
    pub fn get(&self, x: i32, y: i32) -> bool {
        if x < 0
            || y < 0
            || x >= i32::from(max_core::geo::MAP_WIDTH)
            || y >= i32::from(max_core::geo::MAP_HEIGHT)
        {
            return false;
        }

        self.get_index(y as usize * max_core::geo::MAP_WIDTH as usize + x as usize)
    }

    /// Number of set cells (tests/diagnostics).
    pub fn count(&self) -> usize {
        self.words
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum()
    }

    /// Whether nothing is set.
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|word| *word == 0)
    }
}

/// Frame-side vision of the local player's team (see the module docs).
#[derive(Debug, Clone, Default)]
pub struct VisionView {
    /// Live scan coverage of the own units at their visual anchors (current frame).
    ///
    /// There is deliberately **no** cumulative "explored" mask: the original's
    /// `MinimapFov` is rebuilt from the live `HeatMap::complete` counts, so the
    /// fog closes when the last scanner leaves (`access.cpp:576-584/896-910`).
    covered: CellMask,
    /// The visual cell of every own unit (the anchor the coverage was built from).
    anchors: BTreeMap<UnitId, (i16, i16)>,
    /// `true` when the mask describes the **visual** anchors of this frame.
    ///
    /// `false` — the mask is a copy of the simulation (headless/QA frames with
    /// `dt <= 0`, before the first frame, or `GameManager_AllVisible`): the
    /// presentation then reads the simulation values exactly, which keeps the
    /// golden frames byte-identical.
    follow_visual: bool,
}

impl VisionView {
    /// Whether the masks follow the visual anchors (otherwise: the simulation).
    pub const fn follow_visual(&self) -> bool {
        self.follow_visual
    }

    /// The visual anchor of an own unit, if it was recorded (`None` — the unit is
    /// unknown to the frame or the view follows the simulation).
    pub fn anchor(&self, id: UnitId) -> Option<(i16, i16)> {
        self.anchors.get(&id).copied()
    }

    /// Whether `(x, y)` is inside the own units' scan right now.
    pub fn covers(&self, x: i32, y: i32) -> bool {
        self.covered.get(x, y)
    }

    /// Coverage mask of the frame (tests/diagnostics).
    pub fn covered_mask(&self) -> &CellMask {
        &self.covered
    }

    /// Drops the frame state: the next frame reseeds from the simulation.
    pub fn reset(&mut self) {
        self.covered.clear();
        self.anchors.clear();
        self.follow_visual = false;
    }

    /// Copies the **simulation** state — `dt <= 0` (QA/headless) and the first frame.
    ///
    /// `covered` is the player team's live coverage — every heat map cell with
    /// `complete > 0` (`HeatMap::IsVisible`), the exact value the presentation
    /// reads without [`Self::follow_visual`].
    pub fn reset_from_sim(&mut self, state: StateRef<'_>) {
        self.covered.clear();
        self.anchors.clear();
        self.follow_visual = false;

        let player = state.player_team();

        if let Some(map) = state.ai().heat_maps.get(usize::from(player.0)) {
            for (index, cell) in map.cells().iter().enumerate() {
                if cell.complete > 0 {
                    self.covered.set_index(index);
                }
            }
        }
    }

    /// Rebuilds the mask from the frame's visual anchors.
    ///
    /// `anchors` — the visual cell of the own units (a fractional position is
    /// rounded to the cell the sprite stands in, exactly like the renderer picks
    /// the drawing cell). Units without an entry keep their simulation cell.
    ///
    /// The mask is cleared first: it is the **current** scan, not a history
    /// (`Access_UpdateMapStatus(unit, false)` removes the old coverage when the
    /// sprite crosses a cell border, `access.cpp:782-784/847`).
    pub fn rebuild(&mut self, state: StateRef<'_>, anchors: &BTreeMap<UnitId, (i16, i16)>) {
        let player = state.player_team();

        self.covered.clear();
        self.anchors.clear();
        self.follow_visual = true;

        for id in state.unit_ids_in_order() {
            let Some(unit) = state.unit(id) else {
                continue;
            };

            if unit.team != player {
                continue;
            }

            // The anchor: the visual cell if the frame knows it, otherwise the
            // simulation cell (a unit that has not moved yet).
            let anchor = anchors
                .get(&id)
                .copied()
                .unwrap_or((unit.grid.x, unit.grid.y));

            self.anchors.insert(id, anchor);

            let Some(scan) = state.scan_radius(unit) else {
                continue;
            };

            Self::add_scan(&mut self.covered, anchor, scan);
        }
    }

    /// Adds the scan coverage of a unit anchored at `anchor` to `mask`
    /// (`Access_IsWithinScanRange` geometry, `access.cpp:353`).
    fn add_scan(mask: &mut CellMask, anchor: (i16, i16), scan: i32) {
        let point = Point::new(anchor.0, anchor.1);
        let ulx = (i32::from(anchor.0) - scan).max(0);
        let uly = (i32::from(anchor.1) - scan).max(0);
        let lrx = (i32::from(anchor.0) + scan).min(i32::from(max_core::geo::MAP_WIDTH) - 1);
        let lry = (i32::from(anchor.1) + scan).min(i32::from(max_core::geo::MAP_HEIGHT) - 1);

        for y in uly..=lry {
            for x in ulx..=lrx {
                if max_core::is_within_scan_range_at(point, x, y, scan) {
                    mask.set(x, y);
                }
            }
        }
    }
}

/// Whether the frame draws a foreign unit (`DrawMap_RenderUnits` /
/// `DrawMap_RenderMiniMapUnitList`).
///
/// The original stores the answer in `visible_to_team`, which the live heat map
/// maintains: `Access_OnCellRevealed` spots the units of a newly covered cell and
/// `Access_OnCellHidden` (`access.cpp:576-607`) calls `UnitInfo::Draw(team)` for
/// every unit in a cell whose `complete` dropped to zero — `Draw` sets
/// `visible_to_team[team] = false` (`unitinfo.cpp:2842-2870`). A unit that leaves
/// the scan is therefore **not drawn** even though its simulation flag in the
/// port still says `true` (the port keeps the simulation untouched: changing the
/// flag would change the snapshot/CRC).
///
/// The frame therefore draws a foreign unit only while the own units' **current**
/// scan (at the visual anchors) covers its cell:
///
/// * `is_visible_to_team` stays the gate for stealth: a land mine or a commando
///   inside the scan is not spotted by `DrawStealth` and must stay hidden, while
///   a unit spotted by fire outside the scan is hidden as well — the manual rule
///   «видно только то, что реально попадает в зону scan»;
/// * for a `BUILDING` any of its 2×2 cells counts, like
///   `HeatMap::IsVisible(unit)` (`heatmap.cpp:170-181`) behind
///   `Access_IsVisibleOnHeatMap` (`access.cpp:538`).
pub fn presented_visible(state: StateRef<'_>, view: &VisionView, unit: &UnitInfo) -> bool {
    let player = state.player_team();

    if unit.team == player {
        return true;
    }

    if !unit.is_visible_to_team(player) {
        return false;
    }

    if !view.follow_visual() {
        return true;
    }

    covered_now(view, unit)
}

/// Whether the frame's live scan covers the unit's cell (2×2 for buildings).
fn covered_now(view: &VisionView, unit: &UnitInfo) -> bool {
    let x = i32::from(unit.grid.x);
    let y = i32::from(unit.grid.y);

    if unit.flags & unit_flags::BUILDING != 0 {
        view.covers(x, y)
            || view.covers(x + 1, y)
            || view.covers(x, y + 1)
            || view.covers(x + 1, y + 1)
    } else {
        view.covers(x, y)
    }
}

#[cfg(test)]
#[path = "vision_tests.rs"]
mod tests;
