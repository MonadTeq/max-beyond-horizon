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
//! This module therefore keeps a frame-only copy of the player's scan:
//!
//! * [`VisionView::covered`] — the cells covered by the own units' scan at their
//!   **visual** anchor (the cell the sprite stands in);
//! * [`VisionView::explored`] — the cumulative (monotone) union, the minimap fog
//!   memory (`Access_UpdateMinimapFogOfWar`).
//!
//! It never writes to the simulation, is not part of `snapshot_bytes`/CRC and is
//! rebuilt from the simulation for `dt <= 0` (QA/headless frames), so the golden
//! frames stay byte-identical.

use std::collections::BTreeMap;

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

    /// `self |= other`.
    pub fn union_with(&mut self, other: &CellMask) {
        for (word, add) in self.words.iter_mut().zip(other.words.iter()) {
            *word |= *add;
        }
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
    /// Scan coverage of the own units at their visual anchors (current frame).
    covered: CellMask,
    /// Cumulative union of [`Self::covered`] — the minimap fog memory.
    explored: CellMask,
    /// The visual cell of every own unit (the anchor the coverage was built from).
    anchors: BTreeMap<UnitId, (i16, i16)>,
    /// `true` when the masks describe the **visual** anchors of this frame.
    ///
    /// `false` — the masks are a copy of the simulation (headless/QA frames with
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

    /// Whether `(x, y)` was ever covered (the minimap fog is off there).
    pub fn explored(&self, x: i32, y: i32) -> bool {
        self.explored.get(x, y)
    }

    /// Coverage mask of the frame (tests/diagnostics).
    pub fn covered_mask(&self) -> &CellMask {
        &self.covered
    }

    /// Explored mask of the frame (tests/diagnostics).
    pub fn explored_mask(&self) -> &CellMask {
        &self.explored
    }

    /// Drops the frame state: the next frame reseeds from the simulation.
    pub fn reset(&mut self) {
        self.covered.clear();
        self.explored.clear();
        self.anchors.clear();
        self.follow_visual = false;
    }

    /// Copies the **simulation** state — `dt <= 0` (QA/headless) and the first frame.
    ///
    /// `explored` is the player team's heat map (`complete > 0`,
    /// `Access_UpdateMinimapFogOfWar`), `covered` — the cells of the enemies that
    /// the simulation currently reports as visible to the player's team (the exact
    /// value the presentation reads without [`Self::follow_visual`]).
    pub fn reset_from_sim(&mut self, state: StateRef<'_>) {
        self.covered.clear();
        self.explored.clear();
        self.anchors.clear();
        self.follow_visual = false;

        self.seed_explored(state);

        let player = state.player_team();

        // The current coverage: the cells of enemies the simulation shows to the
        // player's team. This is exactly «visible right now» for the frame code
        // that reads the mask in the `!follow_visual` branch.
        for id in state.unit_ids_in_order() {
            let Some(unit) = state.unit(id) else {
                continue;
            };

            if unit.team != player && unit.is_visible_to_team(player) {
                self.covered
                    .set(i32::from(unit.grid.x), i32::from(unit.grid.y));
            }
        }
    }

    /// Seeds [`Self::explored`] with the player team's heat map (`complete > 0`,
    /// `Access_UpdateMinimapFogOfWar`): cells explored before the frame started
    /// following the visual anchors must not go dark again.
    fn seed_explored(&mut self, state: StateRef<'_>) {
        let player = state.player_team();

        if let Some(map) = state.ai().heat_maps.get(usize::from(player.0)) {
            for (index, cell) in map.cells().iter().enumerate() {
                if cell.complete > 0 {
                    self.explored.set_index(index);
                }
            }
        }
    }

    /// Rebuilds the masks from the frame's visual anchors.
    ///
    /// `anchors` — the visual cell of the own units (a fractional position is
    /// rounded to the cell the sprite stands in, exactly like the renderer picks
    /// the drawing cell). Units without an entry keep their simulation cell.
    pub fn rebuild(&mut self, state: StateRef<'_>, anchors: &BTreeMap<UnitId, (i16, i16)>) {
        let player = state.player_team();

        // The first frame after a reset/`dt <= 0` path: the cells explored before
        // must stay explored (the fog is a memory — `Access_UpdateMinimapFogOfWar`).
        if !self.follow_visual {
            self.seed_explored(state);
        }

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

        let covered = self.covered.clone();
        self.explored.union_with(&covered);
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
/// The simulation flag (`visible_to_team`) is the authority. While the view follows
/// the visual anchors, a foreign unit additionally has to be inside the scan of an
/// own unit **at its visual cell** — the radar of a moving unit then reveals the
/// enemy on arrival, not when its simulation cell did (`Access_UpdateMapStatus` is
/// called on the cell crossing in `UnitInfo::Move`, `unitinfo.cpp:1787-1816`).
///
/// A unit that the simulation shows although **no** own unit covers its cell is
/// never hidden: that visibility does not come from the scan but from
/// `UnitInfo::SpotByTeam` on fire (`apply_damage`, `combat.rs:122`) or a revealed
/// mine, and the original keeps such a unit on the map.
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

    let x = i32::from(unit.grid.x);
    let y = i32::from(unit.grid.y);

    if view.covers(x, y) {
        return true;
    }

    // Not covered at the frame's anchors: either a moving scanner is still behind
    // (hide until it arrives) or the cell is not covered by any own scan at all
    // (spotted by fire) — the latter must stay visible.
    !scan_covers_now(state, x, y)
}

/// Whether any own unit's scan covers the cell at its **simulation** cell
/// (`Access_IsWithinScanRange` with the `grid` of `Access_UpdateMapStatus`).
fn scan_covers_now(state: StateRef<'_>, x: i32, y: i32) -> bool {
    let player = state.player_team();

    for id in state.unit_ids_in_order() {
        let Some(unit) = state.unit(id) else {
            continue;
        };

        if unit.team != player {
            continue;
        }

        let Some(scan) = state.scan_radius(unit) else {
            continue;
        };

        if max_core::is_within_scan_range_at(unit.grid, x, y, scan) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_mask_sets_and_reads_cells() {
        let mut mask = CellMask::new();

        assert!(mask.is_empty());
        mask.set(5, 7);
        assert!(mask.get(5, 7));
        assert!(!mask.get(7, 5));
        // Out of the map: nothing is stored and nothing is reported.
        mask.set(-1, 0);
        mask.set(0, i32::from(max_core::geo::MAP_HEIGHT) + 5);
        assert!(!mask.get(-1, 0));
        assert!(!mask.get(0, 200));
        assert_eq!(mask.count(), 1);
    }

    #[test]
    fn cell_mask_union_is_monotone() {
        let mut left = CellMask::new();
        let mut right = CellMask::new();

        left.set(1, 1);
        right.set(2, 2);
        left.union_with(&right);

        assert!(left.get(1, 1));
        assert!(left.get(2, 2));
        assert!(right.get(2, 2) && !right.get(1, 1));
    }

    #[test]
    fn the_scan_coverage_is_a_disc_around_the_anchor() {
        let mut mask = CellMask::new();

        VisionView::add_scan(&mut mask, (10, 10), 2);

        // The anchor cell and the four neighbours are inside; a diagonal corner at
        // the square bound (2,2 cells away) is outside (the `* 64` euclidean check).
        assert!(mask.get(10, 10));
        assert!(mask.get(12, 10));
        assert!(mask.get(10, 12));
        assert!(!mask.get(12, 12));
        assert!(!mask.get(13, 10));
    }
}
