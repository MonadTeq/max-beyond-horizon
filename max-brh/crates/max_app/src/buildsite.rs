//! Build site selection mode — a port of the `ORDER_STATE_SELECT_SITE` constructor
//! (`buildmenu.cpp:921`, `game_manager.cpp:4566/5829`).
//!
//! # What C++ does (oracle)
//!
//! 1. `MobileBuildMenu::Build` (`buildmenu.cpp:872`): the player chose a building
//!    (`Unit::GetFlags() & BUILDING`) and pressed `Done` → `Build`
//!    (`UnitsManager_IssueBuildOrder`) checks four 2×2 anchors around
//!    the builder, then `UnitsManager_SetNewOrderInt(unit, ORDER_BUILD,
//!    ORDER_STATE_SELECT_SITE)` and **spawns a temporary "tape"**
//!    `GameManager_TempTape = UnitsManager_SpawnUnit(LRGTAPE, …, grid_x,
//!    grid_y, unit)` — this is the site ghost (not a building ghost and not
//!    a frame). The message is `_(06cb)`.
//! 2. `GameManager_GetBuilderUnitCursor` (`game_manager.cpp:5829`) on every
//!    mouse move: for the constructor the 2×2 tape is bound to the builder
//!    (`site_anchor_for_cursor`) and moved by `UnitsManager_MoveUnit`;
//!    `CURSOR_MAP2` — if `UnitsManager_MoveUnitAndParent` returned
//!    passability **and** the cursor fell inside the 2×2, otherwise `CURSOR_UNIT_NO_GO`.
//! 3. `GameManager_PathBuild` (`game_manager.cpp:4566`) on LMB:
//!    `move_to = TempTape->grid`, the tape is destroyed, `UnitsManager_StartBuild`
//!    → `ORDER_BUILD`/`INIT`; on the next tick `UnitInfo::StartBuilding`
//!    places the real site (`LRGTAPE`+`LRGCONES`+`LRGSLAB`) at `move_to`.
//! 4. Cancel — `GameManager_ManagePlayerAction` → `GameManager_SelectBuildSite`:
//!    the tape is destroyed, `BuildList` is cleared, the orders are restored.
//!    This is how RMB and (in C++) a click on an invalid cell work.
//! 5. After `StartBuilding` the constructor goes through `ORDER_AWAIT_TAPE_POSITIONING`
//!    (`UnitInfo::PositionInTape`, `unitinfo.cpp:4414`). First
//!    `Paths_UpdateAngle` turns it toward `GetTargetUnitAngle` (for 2×2 —
//!    the diagonal to the center, `unitinfo.cpp:4147`); while the turn is not finished,
//!    `moved` does not grow. Then four ticks of `OffsetDrawZones` by 8 px move
//!    the **sprite** of the builder along `DIRECTION_OFFSETS[angle]` by 32 px (half
//!    a cell) — to the center of the tape, facing the direction of travel. On completion C++ turns
//!    the unit by a step `(angle + 1) & 7` into the working pose. The grid (`grid_x/grid_y`)
//!    does not change.
//!
//! # What is ported
//!
//! * Binding the 2×2 anchor to the builder and validating the cell — `max_core::production`
//!   ([`max_core::production::initial_build_site`],
//!   [`max_core::production::site_anchor_for_cursor`],
//!   [`max_core::production::site_covers`],
//!   [`max_core::production::is_site_accessible`]).
//! * The ghost is **the same decal** `LRGTAPE`/`SMLTAPE` that C++ spawns (drawn by
//!   the common sprite loop), the cursor `CURSOR_MAP2`/`CURSOR_UNIT_NO_GO`
//!   (SDL layer, like `Cursor_SetCursor`).
//! * LMB — order 08/P4 with `move_to` = the ghost anchor; the site
//!   (`LRGTAPE`+`LRGCONES`+`LRGSLAB`) is placed by `events::apply_command` at the moment
//!   the packet is applied — the slab is visible already on the confirmation frame. C++ places it
//!   on the next tick (`StartBuilding`/`INIT`), which is invisible at 60 fps; in
//!   the port `unit_order::start_building` is idempotent and on the next tick
//!   only switches the builder to `PositionInTape`.
//! * RMB/Esc — cancel without an order (the order is not sent).
//! * Shift to the tape center — [`tape_center_offset`]: turning toward the tape
//!   (`unit_order::position_in_tape`), then 8 px × 4 ticks = 32 px along
//!   DIRECTION_OFFSETS[unit.angle] (OffsetDrawZones), applied by the renderer
//!   to the sprite anchor; for `DEINIT`/`LEAVE` — the same 32 px plus 8 px per tick
//!   toward `move_to` (the builder moves off the site before `ActivateEngineer`).
//!
//! # Deliberate differences
//!
//! * C++ spawns a temporary tape in the world (`GameManager_TempTape`); the port draws
//!   it as a UI ghost — the match state, the unit counter, the CRC and the save do not
//!   change while the player chooses a place.
//! * `GetBuilderUnitCursor` for `ENGINEER` (`game_manager.cpp:5854-5909`)
//!   is ported: the 1×1 tape stands in the cursor cell, only in the row/column of
//!   the engineer; enabled by the `Path` button (`BLDPTH_U`, `buildmenu.cpp:836`).
//!   C++ `UnitsManager_StartBuild` (`units_manager.cpp:1516`) resets
//!   the engineer's `move_to` to its cell — that would nullify the `Path` choice, so
//!   the port places the site in the chosen cell (see §33).
//! * C++ opens an exit menu on Esc (`OKCancelMenu`); here Esc cancels the
//!   choice, as the specification asks.
//! * `GameManager_AutoSelectNext` is not called after confirmation (as in
//!   the other port orders).

use max_core::access;
use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::universe::StateRef;
use max_core::geo::{Point, DIRECTION_OFFSETS};
use max_core::production;
use max_core::team::TeamId;
use max_core::unit::{UnitId, UnitInfo};
use max_render::buffer::RgbaBuffer;
use max_render::viewport::Viewport;

use crate::cursor::CursorId;
use crate::local::SessionChannel;

/// Builder sprite shift per one `PositionInTape` tick
/// (`OffsetDrawZones(DIRECTION_OFFSETS[angle] * 8)`, `unitinfo.cpp:4478`).
pub const TAPE_STEP_PIXELS: i32 = 8;
/// How many ticks the positioning takes (`moved >= 4`).
pub const TAPE_STEPS: u8 = 4;
/// Total shift to the tape center: `8 * 4 = 32` px — half a cell (64 px).
pub const TAPE_CENTER_PIXELS: i32 = TAPE_STEP_PIXELS * TAPE_STEPS as i32;

/// Build site selection mode (`ORDER_STATE_SELECT_SITE`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildSiteSelect {
    /// Builder (constructor).
    pub builder: UnitId,
    /// Builder team.
    pub team: TeamId,
    /// Builder type (`CONSTRCT`).
    pub builder_type: UnitType,
    /// What we build (the head of the queue).
    pub unit_type: UnitType,
    /// Build speed (1/2/4) chosen in `BuildMenu`.
    pub build_rate: u16,
    /// 2×2 anchor (the tape cell `GameManager_TempTape`).
    pub anchor: Point,
    /// Cell under the cursor (for status and the hit check).
    pub cursor: Point,
    /// Status line (whether the cell is valid).
    pub status: String,
}

impl BuildSiteSelect {
    /// Enters the mode with the anchor `anchor` (`UnitsManager_IssueBuildOrder`).
    pub fn new(
        builder: UnitId,
        team: TeamId,
        builder_type: UnitType,
        unit_type: UnitType,
        build_rate: u16,
        builder_grid: Point,
        anchor: Point,
    ) -> Self {
        Self {
            builder,
            team,
            builder_type,
            unit_type,
            build_rate,
            anchor,
            cursor: builder_grid,
            status: String::new(),
        }
    }

    /// Temporary tape type (`DeployConstructionSiteUtilities`):
    /// a building — `LRGTAPE`, otherwise `SMLTAPE`.
    pub fn tape_type(&self) -> UnitType {
        production::construction_utility_types(self.unit_type).0
    }

    /// The engineer (`ENGINEER`) places a 1×1 site — `GameManager_GetBuilderUnitCursor`,
    /// the `unit1->GetUnitType() == ENGINEER` branch (`game_manager.cpp:5854`).
    pub fn is_engineer(&self) -> bool {
        self.builder_type == UnitType::Engineer
    }

    /// Moves the ghost after the cursor — `GameManager_GetBuilderUnitCursor`.
    ///
    /// The anchor is recomputed per axis separately and kept when the
    /// cursor is exactly on the builder's line (as in C++). For the engineer the 1×1 tape stands
    /// right in the cursor cell (`target_grid_x = grid_x` in C++).
    pub fn update_cursor(&mut self, state: StateRef<'_>, cursor: Point) {
        self.cursor = cursor;

        if let Some(unit) = state.unit(self.builder) {
            self.anchor = production::site_anchor_for_cursor(unit.grid, cursor, self.anchor);
        }

        if self.is_engineer() {
            self.anchor = cursor;
        }

        self.status.clear();
    }

    /// Anchor for an arbitrary cursor cell (C++
    /// `GameManager_GetBuilderUnitCursor`): binding to the builder keeping the
    /// previous axis when the cursor is exactly on its line; for the engineer — the cell itself.
    pub fn anchor_at(&self, state: StateRef<'_>, cursor: Point) -> Point {
        if self.is_engineer() {
            return cursor;
        }

        match state.unit(self.builder) {
            Some(unit) => production::site_anchor_for_cursor(unit.grid, cursor, self.anchor),
            None => self.anchor,
        }
    }

    /// Whether an arbitrary cursor cell is valid with the anchor `anchor`.
    pub fn is_valid_at(&self, state: StateRef<'_>, anchor: Point, cursor: Point) -> bool {
        let Some(unit) = state.unit(self.builder) else {
            return false;
        };

        if unit.team != self.team {
            return false;
        }

        if self.is_engineer() {
            // `GameManager_GetBuilderUnitCursor` (`game_manager.cpp:5901-5909`):
            // the 1×1 tape only in the engineer's row/column and only on
            // a passable cell (`UnitsManager_MoveUnitAndParent`).
            return anchor == cursor
                && production::engineer_site_covers(unit.grid, cursor)
                && state.is_site_accessible_ignoring(
                    self.team,
                    self.unit_type,
                    cursor,
                    &[self.builder],
                );
        }

        // C++ `GetBuilderUnitCursor` binds the 2×2 to the builder: the anchor is always
        // one of the four covering its cell. The check prevents placing
        // the site "far away" and building remotely, even if the mode state
        // came from another place.
        if !production::site_covers(self.unit_type, anchor, unit.grid) {
            return false;
        }

        production::site_covers(self.unit_type, anchor, cursor)
            && state.is_site_accessible_ignoring(
                self.team,
                self.unit_type,
                anchor,
                &[self.builder],
            )
    }

    /// Whether the current cell is valid: the cursor is inside the 2×2 and the site is passable
    /// (`UnitsManager_MoveUnitAndParent` in C++).
    pub fn valid(&self, state: StateRef<'_>) -> bool {
        self.is_valid_at(state, self.anchor, self.cursor)
    }

    /// Window cursor: `CURSOR_MAP2` over a valid cell, `CURSOR_UNIT_NO_GO`
    /// over an invalid one (`GameManager_GetBuilderUnitCursor`).
    pub fn cursor_id(&self, state: StateRef<'_>) -> CursorId {
        if self.valid(state) {
            CursorId::Map2
        } else {
            CursorId::UnitNoGo
        }
    }

    /// Status line text for the HUD/log.
    pub fn status_line(&self, state: StateRef<'_>) -> String {
        let (x, y) = (self.anchor.x, self.anchor.y);

        if self.valid(state) {
            format!("Construction site: {x},{y} — LMB builds, Esc/RMB cancels")
        } else {
            format!("Cannot build here ({x},{y}) — move the cursor")
        }
    }

    /// Draws the tape ghost into the frame (`GameManager_TempTape`).
    ///
    /// The sprite is the same as the real site: `LRGTAPE`/`SMLTAPE` with the team
    /// colors; frame `0` (the tape is not animated).
    pub fn draw_ghost(
        &self,
        frame: &mut RgbaBuffer,
        assets: &crate::Assets,
        viewport: &Viewport,
    ) -> Option<u32> {
        let tape_type = self.tape_type();
        let mut ghost = UnitInfo::new(tape_type, self.team, UnitId::NO_ID);
        ghost.flags = access::unit_type_flags(tape_type);
        // `IsRelevant` (`unitinfogroup.cpp:59`) draws only non-`ORDER_IDLE`.
        ghost.orders = UnitOrder::Await;
        ghost.state = UnitOrderState::ExecutingOrder;
        ghost.set_visible_to_team(self.team, true);

        let (world_x, world_y) = crate::render::unit_anchor_world(
            &ghost,
            (f32::from(self.anchor.x), f32::from(self.anchor.y)),
        );
        let (screen_x, screen_y) = viewport.world_to_screen(world_x, world_y);
        // The ghost is not a state unit: default C++ presentation
        // (`brightness = 255`, zeros) — phase 5.3/B.
        let presentation = max_core::unit::UnitPresentation::new();
        let drawn = crate::render::draw_unit_sprites(
            assets,
            &ghost,
            &presentation,
            frame,
            (screen_x, screen_y),
            viewport.zoom(),
        )?;

        Some(drawn.unit + drawn.turret)
    }
}

/// Builder sprite shift relative to the center of its cell — a port of
/// `UnitInfo::PositionInTape` (`unitinfo.cpp:4414`) + `OffsetDrawZones`.
///
/// * `AWAIT_TAPE_POSITIONING`/`TAPE_POSITIONING_{INIT,ENTER}` — `8 px` per tick
///   (`moved` 0..4, like the C++ counter), **by the current hull angle**
///   (`DIRECTION_OFFSETS[angle]`): C++ first turns the builder toward the tape
///   (`Paths_UpdateAngle`), so the sprite slides where it faces;
/// * `ORDER_BUILD`/`BUILD_IN_PROGRESS` after the orders are restored —
///   the full `32 px` to the tape center (the working pose is already in the center);
/// * `ORDER_BUILD`/`UNIT_READY` (the building is finished, the builder **waits for a player
///   order**) — the same `32 px`: C++ does not move `x`/`y` when the order changes, so
///   the constructor stays in the site center until it is sent (`TAPE_
///   POSITIONING_DEINIT` — the first shift);
/// * `AWAIT_TAPE_POSITIONING`/`TAPE_POSITIONING_DEINIT` — the builder is still in
///   the tape center (`32 px` of entry), `TAPE_POSITIONING_LEAVE` — the same `32 px`
///   plus `8 px × moved` toward `move_to` (`move_to_grid_*`): this is how C++
///   moves off the site before `ActivateEngineer`;
/// * the other states (completion, `CancelBuilding`, after leaving) — `0`:
///   the sprite in the center of its cell, like `UnitInfo::Redraw`.
///
/// The entry direction is the angle to the tape center (for 2×2 it is one of the four
/// diagonals), so the shift is computed from the site geometry rather than from
/// the current hull angle: the angle could have been turned to the working pose
/// (`Paths_UpdateAngle(this, (angle + 1) & 0x07)`). The builder's grid does not
/// change on entry.
pub fn tape_center_offset(state: StateRef<'_>, unit: &UnitInfo) -> (i32, i32) {
    if unit.unit_type != UnitType::Constructor || !unit.is_mobile() {
        return (0, 0);
    }

    // The site tape is the one whose parent is this builder
    // (`DeployConstructionSiteUtilities`: `tape->SetParent(this)`).
    let tape = state
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| state.unit(id))
        .find(|candidate| {
            candidate.parent_unit == Some(unit.id)
                && matches!(
                    candidate.unit_type,
                    UnitType::LargeTape | UnitType::SmallTape
                )
        });

    let Some(tape) = tape else {
        return (0, 0);
    };

    // The tape center — only if the builder stands inside its 2×2. Otherwise the shift
    // would take the sprite away from the site center (a stale/damaged anchor).
    if !production::site_covers(tape.unit_type, tape.grid, unit.grid) {
        return (0, 0);
    }

    // Entry: the builder is in one of the four cells of the 2×2, the tape center is 32 px
    // toward its center on each axis.
    let (enter_x, enter_y) = if tape.unit_type == UnitType::LargeTape {
        (
            if unit.grid.x == tape.grid.x { 1 } else { -1 },
            if unit.grid.y == tape.grid.y { 1 } else { -1 },
        )
    } else {
        (0, 0)
    };
    let (enter_px_x, enter_px_y) = (enter_x * TAPE_CENTER_PIXELS, enter_y * TAPE_CENTER_PIXELS);

    match (unit.orders, unit.state) {
        (
            UnitOrder::AwaitTapePositioning,
            UnitOrderState::TapePositioningInit | UnitOrderState::TapePositioningEnter,
        ) => {
            let steps = i32::from(unit.moved.min(TAPE_STEPS));
            let direction = DIRECTION_OFFSETS[usize::from(unit.angle & 0x07)];

            (
                i32::from(direction.x) * TAPE_STEP_PIXELS * steps,
                i32::from(direction.y) * TAPE_STEP_PIXELS * steps,
            )
        }
        (UnitOrder::AwaitTapePositioning, UnitOrderState::TapePositioningDeinit) => {
            (enter_px_x, enter_px_y)
        }
        (UnitOrder::AwaitTapePositioning, UnitOrderState::TapePositioningLeave) => {
            let move_to = state.order_data(unit.id).move_to;
            let angle = leave_angle(unit.grid, move_to);
            let direction = DIRECTION_OFFSETS[usize::from(angle & 0x07)];
            let steps = i32::from(unit.moved.min(TAPE_STEPS));

            (
                enter_px_x + i32::from(direction.x) * TAPE_STEP_PIXELS * steps,
                enter_px_y + i32::from(direction.y) * TAPE_STEP_PIXELS * steps,
            )
        }
        (UnitOrder::Build, UnitOrderState::BuildInProgress) => (enter_px_x, enter_px_y),
        // Completion: C++ `SpawnNewUnit` (`unitinfo.cpp:3440-3592`) switches
        // the builder to `ORDER_BUILD`/`ORDER_STATE_UNIT_READY`, but its pixel
        // (`x`/`y`) stays where `PositionInTape` brought it — in the center of
        // the tape. The shift is not removed until the player's order: the first reset happens
        // only in `TAPE_POSITIONING_DEINIT`/`LEAVE`. Previously there was `0` here, and
        // the sprite was "pushed" into the site corner right after completion.
        (UnitOrder::Build, UnitOrderState::UnitReady) => (enter_px_x, enter_px_y),
        // The player's click on the cell: C++ sets `ORDER_ACTIVATE`/
        // `ORDER_STATE_IN_TRANSITION`, but the builder pixel is still in the tape center.
        (UnitOrder::Activate, UnitOrderState::InTransition) => (enter_px_x, enter_px_y),
        _ => (0, 0),
    }
}

/// `GetTargetUnitAngle` for leaving (`state != TAPE_POSITIONING_ENTER`) —
/// the sign of the difference to `move_to` on each axis; the diagonals are encoded by the angles
/// `NE/SE/SW/NW` exactly as in C++.
fn leave_angle(grid: Point, move_to: Point) -> u8 {
    let target_x = i32::from(move_to.x);
    let target_y = i32::from(move_to.y);

    if target_x > i32::from(grid.x) {
        if target_y > i32::from(grid.y) {
            3
        } else {
            1
        }
    } else if target_y > i32::from(grid.y) {
        5
    } else {
        7
    }
}

/// World point of the 2×2 tape center with the anchor `anchor` (for tests/asserts).
pub fn site_center_world(unit_type: UnitType, anchor: Point) -> (i32, i32) {
    let size = i32::from(production::footprint(unit_type));

    (
        i32::from(anchor.x) * 64 + size * 32,
        i32::from(anchor.y) * 64 + size * 32,
    )
}

// ---------------------------------------------------------------------------
// Integration with the application
// ---------------------------------------------------------------------------

impl<D: max_core::universe::Dispatcher> crate::App<D> {
    /// Open build site selection mode; `None` — a regular match.
    pub fn build_site(&self) -> Option<&BuildSiteSelect> {
        self.build_site.as_ref()
    }

    /// Starts the site selection (`MobileBuildMenu::Build` → `SELECT_SITE`).
    ///
    /// `false` — there is no passable 2×2 anchor next to the builder
    /// (`UnitsManager_IssueBuildOrder` returned `false`, `BuildMenu` stays
    /// open with an error message).
    pub fn open_build_site(&mut self, unit_type: UnitType, build_rate: u16) -> bool {
        let Some(menu) = self.build_menu.as_ref() else {
            return false;
        };
        let (builder, team, builder_type) = (menu.unit, menu.team, menu.builder_type);
        let Some(unit) = self.state_ref().unit(builder) else {
            return false;
        };
        let builder_grid = unit.grid;

        let Some(anchor) =
            self.state_ref().initial_build_site( team, unit_type, builder_grid, &[builder])
        else {
            return false;
        };

        self.build_menu = None;
        self.build_site = Some(BuildSiteSelect::new(
            builder,
            team,
            builder_type,
            unit_type,
            build_rate,
            builder_grid,
            anchor,
        ));

        true
    }

    /// Cancels the site selection (`GameManager_SelectBuildSite`): the order is not sent.
    pub fn cancel_build_site(&mut self) -> Option<String> {
        let site = self.build_site.take()?;

        Some(format!(
            "Build cancelled: {} not ordered",
            crate::buildmenu::unit_name(site.unit_type)
        ))
    }

    /// Confirms the site selection (`GameManager_PathBuild` → `ORDER_BUILD`).
    ///
    /// Returns a log line; `None` — no mode. An invalid cell
    /// cancels the selection, as in C++ (`CURSOR_UNIT_NO_GO` → `ManagePlayerAction`).
    pub fn confirm_build_site(&mut self, channel: SessionChannel) -> Option<String> {
        let (valid, anchor) = {
            let site = self.build_site.as_ref()?;

            (site.valid(self.state_ref()), site.anchor)
        };

        if !valid {
            return self
                .cancel_build_site()
                .map(|line| format!("{line} (invalid cell {},{} )", anchor.x, anchor.y));
        }

        let site = self.build_site.take()?;

        // `GameManager_PathBuild` (`game_manager.cpp:4583-4596`): an ENGINEER that
        // was pointed at a cell outside its own starts a **linear `GroundPath`** to
        // that cell (`GroundPath` + `AppendLinearSteps`) and builds the first
        // object right where it stands — `UnitsManager_StartBuild` resets the
        // engineer's `move_to` to its own cell. The path makes
        // `UnitsManager_BuildNext` (`units_manager.cpp:3807`) place the same type
        // on every next cell while raw material lasts.
        let (site_cell, path_command) = if site.is_engineer() {
            let builder_grid = self
                .state_ref()
                .unit(site.builder)
                .map(|unit| unit.grid)
                .unwrap_or(site.anchor);

            if builder_grid != site.anchor {
                let builder = self.state_ref().unit(site.builder).cloned();
                let command = builder.map(|builder| {
                    max_core::Command::Move(max_core::Packet38Move {
                        unit: site.builder,
                        order: builder.orders,
                        state: builder.state,
                        grid: builder_grid,
                        move_to: site.anchor,
                        group_speed: builder.group_speed,
                        path: Some(max_core::MovePathData::Ground(
                            max_core::pathfinding::append_linear_steps(builder_grid, site.anchor),
                        )),
                    })
                });

                (builder_grid, command)
            } else {
                (builder_grid, None)
            }
        } else {
            (site.anchor, None)
        };

        // Packet 38 (path) before the 08/P4 build order: `apply_packet_08` does not
        // touch `unit.path`, so the queue head survives the completion of every
        // building of the route (C++ sends 38 then starts the order locally).
        if let Some(command) = path_command {
            if channel.is_lockstep() {
                self.hud.pending.push(command);
            } else {
                self.apply_immediate(command);
            }
        }

        let command = crate::buildmenu::build_order_command(
            self.state_ref(),
            site.builder,
            site_cell,
            &[site.unit_type],
            false,
            site.build_rate,
        )?;

        if channel.is_lockstep() {
            self.hud.pending.push(command);
        } else {
            // B2/B3 audit: the command channel — the `ORDER_BUILD` command is the
            // only mutation path; the demo applies it immediately because there is
            // no session driver (`apply_now` ≡ `submit` + `tick`, gate
            // `tests/channel_equivalence.rs`).
            self.apply_immediate(command);
        }

        Some(format!(
            "Construction site: {} at ({}, {})",
            crate::buildmenu::unit_name(site.unit_type),
            site.anchor.x,
            site.anchor.y
        ))
    }

    /// Closes the mode without a message (exit from the match/load).
    pub fn close_build_site(&mut self) {
        self.build_site = None;
    }
}

#[cfg(test)]
#[path = "buildsite_tests.rs"]
mod tests;
