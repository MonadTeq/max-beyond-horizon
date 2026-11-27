//! Frame without SDL: map + units in [`RgbaBuffer`].
//!
//! Only the **pure** parts of [`max_render`] are collected here:
//! [`render_map_into`] (tiles/surfaces/resources), [`SpriteRenderer`]
//! (palette, team LUTs, shadows) and [`draw_order`] (unit order by `y`,
//! Р’В§5.4). The SDL layer (`max_render::sdl_window`) is connected exclusively in
//! `src/main.rs` behind the `sdl` feature, so the application and its tests draw frames
//! without a window and without a display.
//!
//! If the assets are not loaded ([`App::init_assets`] was not called) or the type
//! resource is absent from [`crate::assets::UNIT_SPRITES`], the unit is drawn as a square
//! team-colored marker. In the game data there is exactly one such type РІР‚вЂќ `MASTER`
//! (`UNIT_TYPES_WITHOUT_SPRITE_DATA`); the map is always drawn with
//! the real surface/resource colors from [`TileStyle`].
//!
//! The common unit drawing point РІР‚вЂќ [`draw_unit_sprites`]: it is used by the map
//! (`App::draw_units`) and the HUD portrait (`panels::draw_portrait`), so all
//! 93 types are drawn the same way (body, shadow, turret with an offset).

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::hd_chrome::scaled_pixel_rect_by;
use max_render::map_render::{render_map_into, TileStyle};
use max_render::palette::{ColorLut, TeamColors, FULL_BRIGHTNESS_LEVEL};

use max_render::sprite_atlas::SpriteRenderer;
use max_render::terrain::PaletteRgba;
use max_render::viewport::{draw_order, Viewport};

use max_core::enums::{unit_flags, TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::unit::UnitPresentation;
use max_core::universe::{Dispatcher, StateRef};

use crate::hd::HdPresenter;
use crate::panels::UiPalette;
use crate::App;

/// `COLOR_GREEN` (`color.h:38`) РІР‚вЂќ a route cell reachable within this turn.
pub const PATH_MARKER_GREEN_INDEX: u8 = 0x02;

/// `COLOR_BLUE` (`color.h:39`) РІР‚вЂќ a route cell of a following turn.
pub const PATH_MARKER_BLUE_INDEX: u8 = 0x03;

/// Draws the route preview arrows of the selected unit РІР‚вЂќ `Paths_DrawMarker`
/// (`paths.cpp:376-395`) for every marker of `GroundPath::Draw`/`AirPath::Draw`.
///
/// Each marker is a triangle of three Bresenham lines through the direction table
/// [`max_core::unit_order::PATH_MARKER_POINTS`]; the vertices are scaled with the zoom
/// exactly like C++ (`(offset << 16) / Gfx_MapScalingFactor`, i.e. `offset * zoom / 64`).
/// Green РІР‚вЂќ the unit reaches the cell this turn, blue РІР‚вЂќ in a later turn.
///
/// Returns the number of drawn markers (diagnostics).
pub fn draw_path_markers(
    buffer: &mut RgbaBuffer,
    viewport: &Viewport,
    markers: &[max_core::unit_order::PathMarker],
    palette: &PaletteRgba,
) -> usize {
    let green = Rgba::from_rgb([
        palette[usize::from(PATH_MARKER_GREEN_INDEX)][0],
        palette[usize::from(PATH_MARKER_GREEN_INDEX)][1],
        palette[usize::from(PATH_MARKER_GREEN_INDEX)][2],
    ]);
    let blue = Rgba::from_rgb([
        palette[usize::from(PATH_MARKER_BLUE_INDEX)][0],
        palette[usize::from(PATH_MARKER_BLUE_INDEX)][1],
        palette[usize::from(PATH_MARKER_BLUE_INDEX)][2],
    ]);

    let mut drawn = 0;

    for marker in markers {
        let Some(points) = max_core::unit_order::PATH_MARKER_POINTS.get(usize::from(marker.angle))
        else {
            continue;
        };

        let color = if marker.this_turn { green } else { blue };
        let mut screen = [(0i32, 0i32); 3];

        for (slot, offset) in screen.iter_mut().zip(points) {
            *slot = viewport.world_to_screen(marker.x + offset.0, marker.y + offset.1);
        }

        // `draw_line(a, b)`, `draw_line(b, c)`, `draw_line(c, a)`.
        crate::hud::draw_line(buffer, screen[0], screen[1], color);
        crate::hud::draw_line(buffer, screen[1], screen[2], color);
        crate::hud::draw_line(buffer, screen[2], screen[0], color);
        drawn += 1;
    }

    drawn
}

/// [`draw_path_markers`] for the procedural HUD frame, which has no game palette in
/// scope: the same triangles, colored with the fixed HUD preview colors.
pub fn draw_path_markers_hud(
    buffer: &mut RgbaBuffer,
    viewport: &Viewport,
    markers: &[max_core::unit_order::PathMarker],
) -> usize {
    let mut drawn = 0;

    for marker in markers {
        let Some(points) = max_core::unit_order::PATH_MARKER_POINTS.get(usize::from(marker.angle))
        else {
            continue;
        };

        let color = if marker.this_turn {
            crate::hud::PREVIEW_COLOR
        } else {
            crate::hud::PREVIEW_COLOR_BLUE
        };
        let mut screen = [(0i32, 0i32); 3];

        for (slot, offset) in screen.iter_mut().zip(points) {
            *slot = viewport.world_to_screen(marker.x + offset.0, marker.y + offset.1);
        }

        crate::hud::draw_line(buffer, screen[0], screen[1], color);
        crate::hud::draw_line(buffer, screen[1], screen[2], color);
        crate::hud::draw_line(buffer, screen[2], screen[0], color);
        drawn += 1;
    }

    drawn
}

/// Whether `GameManager_RenderScanRangeIndicators` (`game_manager.cpp:936-951`)
/// draws the route of this unit: the selected unit is visible to the local team,
/// belongs to it, and is executing a move (or a build) with a path.
pub fn draws_unit_path(unit: &max_core::unit::UnitInfo, team: max_core::team::TeamId) -> bool {
    if unit.path.is_none() || unit.team != team || !unit.is_visible_to_team(team) {
        return false;
    }

    matches!(
        unit.state,
        UnitOrderState::ExecutingOrder
            | UnitOrderState::MoveGettingPath
            | UnitOrderState::ReadyToExecuteOrder
    ) || unit.orders == UnitOrder::Build
}

/// Shift of the building (`flags & BUILDING`) anchor from the cell center.
///
/// C++ `UnitInfo::SetPosition` (`unitinfo.cpp:1301`): `x = grid*64 + 32`, then
/// for `flags & BUILDING` РІР‚вЂќ `x += (GFX_MAP_TILE_SIZE / 2) - 1`, i.e. **+31**.
/// A 2Р“вЂ”2 building with hotspot `(58,55)` (`MININGST`) is drawn in its 2Р“вЂ”2
/// rectangle, not half a cell up-left. Without this the mine "drifted"
/// onto the generator cell and their sprites overlapped.
pub const BUILDING_ANCHOR_OFFSET: i32 = (64 / 2) - 1;

/// Shift of the builder frame into the working pose (`DrawSpriteFrame(image_index + 16)`).
///
/// C++ `UnitInfo::StartBuilding` (`unitinfo.cpp:2789`) for the engineer and
/// `UnitInfo::PositionInTape` (`unitinfo.cpp:4451`) for the constructor. In
/// the `ENGINEER` sprites there are 32 frames, `CONSTRCT` РІР‚вЂќ 40, so `angle + 16` always
/// falls into the working animation block.
pub const BUILDER_WORK_FRAME_OFFSET: usize = 16;

/// Step of the constructor working cycle (`DrawSpriteFrame(image_index + 8)`).
///
/// C++ `UnitsManager_ProcessOrderBuild` (`units_manager.cpp:2856`): while the order is
/// `ORDER_BUILD`/`ORDER_STATE_BUILD_IN_PROGRESS`, the constructor's frame shifts
/// by `+8` every second tick; if `image_index + 8 >= 40`, instead of
/// that `image_index - 16` is taken РІР‚вЂќ i.e. the frames loop
/// `+16 РІвЂ вЂ™ +24 РІвЂ вЂ™ +32 РІвЂ вЂ™ +16`. For the engineer the cycle does not advance: it keeps `+16`.
pub const BUILDER_WORK_FRAME_STEP: usize = 8;

/// How many different frames are in the constructor working cycle (`+16`, `+24`, `+32`).
pub const BUILDER_WORK_FRAME_STEPS: u8 = 3;

/// Whether the mine drill frames `1..15` are advanced.
///
/// **There are no drill frames, so this cannot be enabled.** `MININGST` has 16 body
/// frames in `MAX.RES`, but they are **eight clan variants Р“вЂ” two state frames**:
/// C++ `UnitInfo::UnitInfo` (`unitinfo.cpp:281-283`) sets
/// `image_base = (team_clan - 1) * sizeof(uint16_t)`, i.e. frames `0/1`, `2/3`, РІР‚В¦,
/// `14/15` are the same mine body with the eight clan emblems (`image_base` РІР‚вЂќ off,
/// `+1` РІР‚вЂќ on). All 16 frames have an identical alpha mask; only the palette indices
/// change (the emblem recolour and the on-state beacon/indicator slots). Frames
/// `16..23` are complex connectors (`D_MINING.connector_image_base = 16`), not
/// drilling particles. The DOS capture (`max-original-dasm/formats/unit-animation.md`
/// Р’В§7.2) saw no `0 РІвЂ вЂ™ 1 РІвЂ вЂ™ РІР‚В¦ РІвЂ вЂ™ 15` transition in ~118 s; the visible activity of a
/// working mine is the palette cycle of the drill indicator (slots 21РІР‚вЂњ24,
/// `World::InitColorCycles`, Р’В§7.3). Advancing this block would repaint the mine
/// with another clan's emblem, so the switch stays `false`. **Rust-port artifact, not
/// a C++/DOS feature: do NOT enable without a separate owner decision** (oracle-true,
/// SDL/UI C1/C2 2026-11-24).
pub const MINING_DRILL_FRAMES_ENABLED: bool = false;

/// Unit frame clock (drawing only, not part of the snapshot/CRC).
///
/// Three independent counters repeat three different C++ mechanisms:
///
/// * `builder_step` РІР‚вЂќ `UnitsManager_ProcessOrderBuild` (`+8`, `-16` when
///   `image_index + 8 >= 40`);
/// * `sentry_step` РІР‚вЂќ `UnitsManager_ProcessOrderSentry` for units with the
///   `ANIMATED` flag: every tick `image_index + 1`, after the last РІР‚вЂќ
///   `image_index = image_base`;
/// * `turret_step` РІР‚вЂќ `UnitInfo::SpinningTurretAdvanceAnimation`
///   (`unitinfo.cpp:3891`), called from `UnitsManager_ProcessOrder`
///   (`units_manager.cpp:3263-3266`) for `SPINNING_TURRET` units: every tick
///   `turret_image_index + 1`, after `image_index_max` РІР‚вЂќ `turret_image_base`
///   (the rotating radar of `AWAC`/`SCANNER`).
///
/// There are no **factory** frames here: C++ does not advance them by time. A factory with
/// `ORDER_BUILD` keeps frame `image_base + 1` (`UnitInfo::StartBuilding`,
/// `unitinfo.cpp:2789`), after `SpawnNewUnit`/`CancelBuilding` РІР‚вЂќ `image_base`;
/// the visible "blinking" of a powered-on factory comes from **palette cycles**
/// (`World::InitColorCycles`, `world.cpp:542`: slots 9РІР‚вЂњ30, 6/9/2 fps), not from
/// sprite frames (DOS capture Р’В§7.2/Р’В§7.3 and the `MAX.RES` frames:
/// `TRAINHAL`/`LIGHTPLT`/`AIRPLT` have exactly 2 body frames).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameAnimation {
    /// Number of the builder working cycle frame (`0..BUILDER_WORK_FRAME_STEPS`).
    pub builder_step: u8,
    /// `ProcessOrderSentry` tick number (ANIMATED).
    pub sentry_step: u32,
    /// `SpinningTurretAdvanceAnimation` tick number (`SPINNING_TURRET`).
    pub turret_step: u32,
}

/// Whether the builder frame-by-frame animation is active (`ProcessOrderBuild`).
///
/// C++ advances frames only for `CONSTRCT` in `ORDER_BUILD` /
/// `ORDER_STATE_BUILD_IN_PROGRESS` and only with effects enabled
/// (`ResourceManager_GetSettings()->GetNumericValue("effects")`). The engineer at this
/// time keeps the working pose `+16` without a cycle.
pub fn builder_animation_active(unit: &max_core::unit::UnitInfo) -> bool {
    unit.unit_type == UnitType::Constructor
        && unit.orders == UnitOrder::Build
        && unit.state == UnitOrderState::BuildInProgress
}

/// Whether the `UnitsManager_ProcessOrderSentry` frame-by-frame animation is active.
///
/// C++ (`units_manager.cpp:3029-3041`) advances frames **only** for units with
/// the `ANIMATED` flag and only in the `ORDER_SENTRY` order:
///
/// ```text
/// if (image_index + 1 <= image_index_max) image_index += 1;
/// else                                   image_index = image_base;
/// ```
///
/// Of the buildings this flag is present for `RADAR` (`SC_UNITS`: `ANIMATED`); mines and
/// factories do not have it РІР‚вЂќ C++ does not animate them by time.
pub fn sentry_animation_active(unit: &max_core::unit::UnitInfo) -> bool {
    unit.flags & unit_flags::ANIMATED != 0 && unit.orders == UnitOrder::Sentry
}

/// Whether the `SpinningTurretAdvanceAnimation` turret cycle is active.
///
/// C++ `UnitsManager_ProcessOrder` (`units_manager.cpp:3263-3266`) advances the
/// turret frame of **every** `SPINNING_TURRET` unit on each order tick, for any
/// order except `ORDER_DISABLE` and only with `settings["effects"]` enabled:
///
/// ```text
/// if (turret_image_index + 1 > image_index_max) turret_image_index = turret_image_base;
/// else                                          turret_image_index += 1;
/// ```
///
/// `image_index_max` for these units is `turret_image_base + turret_image_count - 1`
/// (`UnitsManager_DeployUnit`, `units_manager.cpp:2446-2449`), so the dish cycles
/// through exactly the `turret_image_count` frames of `D_*`: `AWAC` РІР‚вЂќ 30,
/// `SCANNER` РІР‚вЂќ 16. The flag is present on both in `SC_UNITS` (and on no building).
pub fn spinning_turret_animation_active(unit: &max_core::unit::UnitInfo) -> bool {
    unit.flags & unit_flags::SPINNING_TURRET != 0 && unit.orders != UnitOrder::Disable
}

/// Turret frame index of a `SPINNING_TURRET` unit for `animation.turret_step`.
///
/// The C++ sequence starts at the frame written by `UpdateTurretAngle`
/// (`turret_image_base + turret_angle`, `unitinfo.cpp:2690-2696`) РІР‚вЂќ for the
/// `AWAC`/`SCANNER` radars `turret_angle` stays 0, because these types never aim
/// (no `TURRET_SPRITE`, `UpdateAngle` does not touch the turret) РІР‚вЂќ then goes
/// `+1` per tick and wraps to `turret_image_base` after `turret_image_count`
/// frames. `None` РІР‚вЂќ the `D_*` header declares no turret frames.
pub fn spinning_turret_frame_index(
    info: &max_assets::sprite::FrameInfo,
    unit: &max_core::unit::UnitInfo,
    animation: FrameAnimation,
) -> Option<usize> {
    let count = usize::from(info.turret_image_count);
    if count == 0 {
        return None;
    }

    let phase = (usize::from(unit.turret_angle % 8) + animation.turret_step as usize) % count;

    Some(usize::from(info.turret_image_base) + phase)
}

/// Whether a **stationary factory** is working РІР‚вЂќ C++ keeps its frame
/// `image_base + 1` (`UnitInfo::StartBuilding`, `unitinfo.cpp:2789`).
///
/// C++ does not advance factory frames by time: `TRAINHAL`/`LIGHTPLT`/`LANDPLT`/
/// `AIRPLT`/`SHIPYARD` in `MAX.RES` have exactly two body frames (`image_base` РІР‚вЂќ
/// off, `image_base + 1` РІР‚вЂќ working) and 8 connector frames. The change of
/// `image_base` РІвЂ вЂќ `image_base + 1` follows the order: `StartBuilding` draws
/// `+1`, `SpawnNewUnit`/`CancelBuilding`/`PowerDown` РІР‚вЂќ the base frame. The visible
/// "blinking" comes from palette cycles (`World::InitColorCycles`), not from frames
/// (`max-original-dasm/formats/unit-animation.md` Р’В§7).
pub fn factory_on_frame_active(unit: &max_core::unit::UnitInfo) -> bool {
    max_core::production::is_factory(unit.unit_type)
        && unit.orders == UnitOrder::Build
        && !matches!(
            unit.state,
            UnitOrderState::UnitReady | UnitOrderState::BuildCancel | UnitOrderState::BuildAbort
        )
}

// ---------------------------------------------------------------------------
// Building connectors (`UnitInfo::RenderWithConnectors`)
// ---------------------------------------------------------------------------

/// Connector frames: the `CONNECTOR_*` bit РІвЂ вЂ™ offset from `connector_image_base`.
///
/// The literal order of `UnitInfo::RenderWithConnectors` (`unitinfo.cpp:3151`):
/// first north/south (two connectors each for a 2Р“вЂ”2 building), then east/west.
/// The offsets are the same as in C++: `CONNECTOR_NORTH_LEFT РІвЂ вЂ™ +0`,
/// `NORTH_RIGHT РІвЂ вЂ™ +4`, `SOUTH_LEFT РІвЂ вЂ™ +2`, `SOUTH_RIGHT РІвЂ вЂ™ +6`,
/// `EAST_TOP РІвЂ вЂ™ +1`, `EAST_BOTTOM РІвЂ вЂ™ +5`, `WEST_TOP РІвЂ вЂ™ +3`, `WEST_BOTTOM РІвЂ вЂ™ +7`.
///
/// It is these frames that the DOS capture took for the mine's "8 drilling particles"
/// (`unit-animation.md` Р’В§7.1): `D_MINING.connector_image_base = 16`,
/// `connector_image_count = 8`, and in a dense base all eight bits were set
/// at the mine. For `POWGEN` (`D_SMLBLD`: base 2, 4 connectors) РІР‚вЂќ four
/// "smoke puffs", actually N/E/S/W pipes.
pub const CONNECTOR_FRAMES: [(u16, usize); 8] = [
    (max_core::enums::CONNECTOR_NORTH_LEFT, 0),
    (max_core::enums::CONNECTOR_NORTH_RIGHT, 4),
    (max_core::enums::CONNECTOR_SOUTH_LEFT, 2),
    (max_core::enums::CONNECTOR_SOUTH_RIGHT, 6),
    (max_core::enums::CONNECTOR_EAST_TOP, 1),
    (max_core::enums::CONNECTOR_EAST_BOTTOM, 5),
    (max_core::enums::CONNECTOR_WEST_TOP, 3),
    (max_core::enums::CONNECTOR_WEST_BOTTOM, 7),
];

/// Connector frame indices for the `connectors` mask in C++ drawing order.
///
/// Empty if the mask is zero, the type has no `connector_image_base`/`count`, or
/// the frame goes beyond the tape length.
pub fn connector_frame_indices(sprite: &crate::assets::LoadedSprite, mask: u16) -> Vec<usize> {
    let Some(info) = sprite.frame_info else {
        return Vec::new();
    };

    if mask == 0 || info.connector_image_count == 0 {
        return Vec::new();
    }

    let base = usize::try_from(info.connector_image_base.max(0)).unwrap_or(0);
    let count = usize::from(info.connector_image_count);
    let mut frames = Vec::new();

    for (bit, offset) in CONNECTOR_FRAMES {
        if mask & bit == 0 || offset >= count {
            continue;
        }

        let index = base + offset;

        if sprite.sheet.frame(index).is_some() {
            frames.push(index);
        }
    }

    frames
}

/// Team building in a cell РІР‚вЂќ `Access_GetTeamBuilding` (`access.cpp:2157`).
///
/// C++ looks for the first unit of the cell with the same team, not `ORDER_IDLE`, and
/// `IsComplexMember()`; `Hash_MapHash` registers all four cells for 2Р“вЂ”2 buildings,
/// so a query by any of them finds the building.
fn team_building_at(
    state: StateRef<'_>,
    team: max_core::team::TeamId,
    x: i16,
    y: i16,
) -> Option<&max_core::unit::UnitInfo> {
    state
        .units_at(max_core::geo::Point::new(x, y))
        .iter()
        .filter_map(|id| state.unit(*id))
        .find(|unit| {
            unit.team == team && unit.orders != UnitOrder::Idle && unit.is_complex_member()
        })
}

/// Building connector mask РІР‚вЂќ **for the frame** (duplicates the simulation
/// `complex::update_connectors`).
///
/// C++ computes the bits when the complex is built/rebuilt and stores them in
/// `UnitInfo::connectors` (in the port this is a simulation field
/// [`max_core::complex::update_connectors`], saved in V70 and included in the
/// snapshot/CRC). This function repeats the same formula over the map **only for
/// the frame**: it does not write to the state. The real mask is merged with it in
/// [`drawn_connector_mask`] РІР‚вЂќ this way the loaded V70 saves, where the bits are already set,
/// and a fresh simulation are drawn the same.
///
/// Rules (exactly as in C++): the neighbor above `(gx, gy-1)` gives `NORTH_LEFT`,
/// above-right `(gx+1, gy-1)` РІР‚вЂќ `NORTH_RIGHT`; below `(gx, gy+size)` РІР‚вЂќ
/// `SOUTH_LEFT`, below-right `(gx+1, gy+size)` РІР‚вЂќ `SOUTH_RIGHT`; right
/// `(gx+size, gy)` РІР‚вЂќ `EAST_TOP`, right-below `(gx+size, gy+1)` РІР‚вЂќ
/// `EAST_BOTTOM`; left `(gx-1, gy)` РІР‚вЂќ `WEST_TOP`, left-below
/// `(gx-1, gy+1)` РІР‚вЂќ `WEST_BOTTOM`. 1Р“вЂ”1 buildings (`POWGEN`) have only
/// `NORTH_LEFT`/`SOUTH_LEFT`/`EAST_TOP`/`WEST_TOP`.
pub fn connector_mask_for(state: StateRef<'_>, unit: &max_core::unit::UnitInfo) -> u16 {
    use max_core::enums::{
        CONNECTOR_EAST_BOTTOM, CONNECTOR_EAST_TOP, CONNECTOR_NORTH_LEFT, CONNECTOR_NORTH_RIGHT,
        CONNECTOR_SOUTH_LEFT, CONNECTOR_SOUTH_RIGHT, CONNECTOR_WEST_BOTTOM, CONNECTOR_WEST_TOP,
    };

    if !unit.is_complex_member() {
        return 0;
    }

    let size: i16 = if unit.is_quad() { 2 } else { 1 };
    let (gx, gy) = (unit.grid.x, unit.grid.y);
    let team = unit.team;
    let has = |x: i16, y: i16| team_building_at(state, team, x, y).is_some();
    let mut mask = 0u16;

    if has(gx, gy - 1) {
        mask |= CONNECTOR_NORTH_LEFT;
    }
    if has(gx, gy + size) {
        mask |= CONNECTOR_SOUTH_LEFT;
    }
    if size == 2 {
        if has(gx + 1, gy - 1) {
            mask |= CONNECTOR_NORTH_RIGHT;
        }
        if has(gx + 1, gy + size) {
            mask |= CONNECTOR_SOUTH_RIGHT;
        }
    }
    if has(gx + size, gy) {
        mask |= CONNECTOR_EAST_TOP;
    }
    if size == 2 && has(gx + size, gy + 1) {
        mask |= CONNECTOR_EAST_BOTTOM;
    }
    if has(gx - 1, gy) {
        mask |= CONNECTOR_WEST_TOP;
    }
    if size == 2 && has(gx - 1, gy + 1) {
        mask |= CONNECTOR_WEST_BOTTOM;
    }

    mask
}

/// The mask the connectors are actually drawn with: stored РІв‚¬Р„ computed.
///
/// The stored mask РІР‚вЂќ from the V70 save/simulation (`UnitInfo::connectors`,
/// [`max_core::complex::update_connectors`]); the computed one РІР‚вЂќ [`connector_mask_for`]
/// (in case the state was loaded without recomputing connectors).
pub fn drawn_connector_mask(state: StateRef<'_>, unit: &max_core::unit::UnitInfo) -> u16 {
    unit.connectors | connector_mask_for(state, unit)
}

/// "Growth" scale from `scaler_adjust`: `2^-scaler` (`UnitInfo::RenderSprite`).
///
/// C++ `UnitInfo::RenderSprite`/`RenderShadow` (`unitinfo.cpp:3085/3050`)
/// divide the frame size by `scaling_factor = 1 << (scaler_adjust + 1)`, i.e.
/// relative to a normal frame (`scaler_adjust = 0`, divisor 2) РІР‚вЂќ `1 / 2^n`:
/// `4 РІвЂ вЂ™ 1/16`, `3 РІвЂ вЂ™ 1/8`, `2 РІвЂ вЂ™ 1/4`, `1 РІвЂ вЂ™ 1/2`, `0 РІвЂ вЂ™ 1`. `0` РІР‚вЂќ full size.
pub fn scaler_scale(scaler_adjust: u8) -> f32 {
    let scaler = scaler_adjust.min(4);
    1.0 / f32::from(1u16 << scaler)
}

/// Body frame base taking the clan into account.
///
/// C++ `UnitInfo::UnitInfo` (`unitinfo.cpp:281`): for a mine
/// `image_base = (team_clan - 1) * sizeof(uint16_t)` РІР‚вЂќ 8 clan variants of
/// 2 frames (off/on). The other types take the base from `D_*` (`unit.image_base`).
/// `TeamClan::Random` (0) is not resolved to a specific clan in max_core, so
/// for it and for saves with a zero base clan 1 (base 0) is used РІР‚вЂќ as it was
/// drawn before.
pub fn image_base_for(unit: &max_core::unit::UnitInfo, team_clan: Option<TeamClan>) -> i16 {
    if unit.unit_type == UnitType::MiningStation && unit.image_base <= 0 {
        let clan = team_clan.map(|clan| clan as i16).unwrap_or(0);
        let clan = if (1..=8).contains(&clan) { clan } else { 1 };

        (clan - 1) * 2
    } else {
        unit.image_base
    }
}

/// Body frame index: `image_base + angle`, and for a powered-on building РІР‚вЂќ +1.
///
/// C++ `UnitsManager_ProcessOrderPowerOn` (`units_manager.cpp:2945`) with
/// `ORDER_POWER_ON` draws `DrawSpriteFrame(image_base + 1)`, `PowerDown` РІР‚вЂќ
/// `DrawSpriteFrame(image_base)` (`unitinfo.cpp:5132`). This is not frame-by-frame
/// animation by time: buildings have exactly two state frames (off/
/// on), the other frames are connectors/directions.
pub fn unit_frame_index(
    unit: &max_core::unit::UnitInfo,
    image_base: i16,
    image_count: usize,
) -> usize {
    unit_frame_index_for_angle(unit, image_base, image_count, unit.angle)
}

/// [`unit_frame_index`] with an explicit body angle.
///
/// The unit sprite РІР‚вЂќ `image_base + angle` (8 directions), so the frame must
/// be chosen by the angle with which the sprite actually moves on the map
/// (`App::visual_angle`), not only by the simulation one: the visual route
/// lags behind the simulation, and otherwise the frame would turn before the turn cell.
///
/// The bridge is the exception: its frames are the **elevation states**
/// (`UnitInfo::IsBridgeElevated` РІР‚вЂќ `image_index != image_base`,
/// `unitinfo.cpp:3030`; `ORDER_STATE_ELEVATE`/`LOWER` walk `image_index` one
/// frame at a time through the `BRIDGE` sprite). C++ renders the stored
/// `image_index` (`UnitInfo::Render`, `unitinfo.cpp:3141`), so the raised bridge
/// must be drawn with it РІР‚вЂќ otherwise the animation never shows and the bridge
/// looks lowered while a ship passes under.
///
/// During construction C++ draws the builder working pose:
/// `DrawSpriteFrame(image_index + 16)` (`unitinfo.cpp:2789` for the engineer and
/// `4451` for the constructor), and `UnitsManager_ProcessOrderBuild` for the constructor
/// additionally advances `+8` (`units_manager.cpp:2856-2868`). The sprites
/// `ENGINEER` (32 frames) and `CONSTRCT` (40) contain these frames, so here
/// exactly `+16` is added while the order is `ORDER_BUILD`, the state РІР‚вЂќ
/// `ORDER_STATE_BUILD_IN_PROGRESS`. After completion (`ORDER_AWAIT`) the frame is
/// base again: C++ returns `DrawSpriteFrame(image_index - 16)`.
pub fn unit_frame_index_for_angle(
    unit: &max_core::unit::UnitInfo,
    image_base: i16,
    image_count: usize,
    angle: u8,
) -> usize {
    let last = image_count.saturating_sub(1);

    // `UnitInfo::IsBridgeElevated` РІР‚вЂќ the stored elevation frame.
    if max_core::unit_order::is_bridge_elevated(unit) {
        return usize::try_from(unit.image_index.max(0))
            .unwrap_or(0)
            .min(last);
    }

    let base = usize::try_from(image_base.max(0)).unwrap_or(0);
    // C++ `UnitInfo::UnitInfo` (`unitinfo.cpp:285`): `image_index = image_base + angle`
    // and `UnitInfo::Render` draws exactly `GetImageIndex()`. For every 8-direction
    // unit `angle` is `0..7`; projectiles keep the **16-direction** firing angle
    // (`D_ROCKET` 16 frames, `D_ALNMISSL` 32), so the angle must not be masked to
    // three bits РІР‚вЂќ `image_base + angle % 8` drew a missile fired south (8) with the
    // north frame. `min(last)` still clips a type with fewer frames than the angle.
    let mut index = base.saturating_add(usize::from(angle));

    // C++ `UnitsManager_ProcessOrderPowerOn` (`units_manager.cpp:2945`) and
    // `UnitInfo::StartBuilding` (`unitinfo.cpp:2789`) draw `image_base + 1`
    // (a powered-on building/working factory), `PowerDown`/`SpawnNewUnit`/
    // `CancelBuilding` РІР‚вЂќ `image_base`.
    if index < last && (unit.orders == UnitOrder::PowerOn || factory_on_frame_active(unit)) {
        index += 1;
    }

    if matches!(unit.unit_type, UnitType::Engineer | UnitType::Constructor)
        && unit.orders == UnitOrder::Build
        && unit.state == UnitOrderState::BuildInProgress
    {
        index = index.saturating_add(BUILDER_WORK_FRAME_OFFSET);
    }

    index.min(last)
}

/// Whether the shot flash (recoil) is active.
///
/// C++ `UnitInfo::PrepareFire` (`unitinfo.cpp:4709`) sets
/// `firing_recoil_frames` (3; Р“вЂ”5 for `ANTIAIR`, Р“вЂ”2 for `SP_FLAK`/`FASTBOAT`;
/// 8 for `COMMANDO`/`INFANTRY`) and draws the firing frame; each
/// `UnitInfo::ProgressFire` (`:4824`) decrements the counter and at zero returns
/// the normal frame. The counter is a state field, so the flash is deterministic and
/// part of the snapshot/CRC.
///
/// Both functions are reached only from `UnitsManager_ProcessOrderFire`
/// (`units_manager.cpp:2781-2812`), i.e. while the unit is in the fire sequence:
/// `orders == ORDER_FIRE` and `state == ORDER_STATE_FIRE_IN_PROGRESS`. Outside it the
/// C++ renderer draws the stored `image_index`/`turret_image_index` and never looks at
/// the counter. The V70 save, however, carries a single `recoil_delay` byte that
/// `UnitInfo::FileLoad` (`unitinfo.cpp:2461-2473`) restores straight into
/// `firing_recoil_frames`; the scenario mission files hold arbitrary non-zero values
/// there (1РІР‚В¦255) for units outside the fire sequence. Deriving the frame from the
/// counter alone РІР‚вЂќ the port does not draw the saved `image_index` РІР‚вЂќ painted the shot
/// flash over those units from the first frame of a mission. The order/state gate is
/// what keeps the flash inside the actual firing sequence, exactly as in C++.
pub fn firing_frame_active(unit: &max_core::unit::UnitInfo) -> bool {
    unit.orders == UnitOrder::Fire
        && unit.state == UnitOrderState::FireInProgress
        && unit.firing_recoil_frames > 0
        && unit.flags & unit_flags::HAS_FIRING_SPRITE != 0
}

/// The **body** frame during recoil (`UnitInfo::PrepareFire`).
///
/// * `COMMANDO`/`INFANTRY` РІР‚вЂќ the special 8-frame C++ queue: `DrawSpriteFrame(
///   unit_angle + 104)`, then every `ProgressFire` РІР‚вЂќ `image_index + 8`
///   (unitinfo.cpp:4732-4742/4831-4832), i.e. the frame
///   `104 + angle + 8Р’В·(8 РІв‚¬вЂ™ firing_recoil_frames)`;
/// * other units without a turret (`flags & TURRET_SPRITE == 0`) РІР‚вЂќ
///   `DrawSpriteFrame(unit_angle + firing_image_base)`;
/// * units with a turret keep the body unchanged (C++ draws only the turret).
///
/// `None` РІР‚вЂќ the body is drawn with the normal frame ([`unit_frame_index_with_animation`]).
pub fn firing_body_frame_index(
    info: &max_assets::sprite::FrameInfo,
    unit: &max_core::unit::UnitInfo,
    body_angle: u8,
) -> Option<usize> {
    if !firing_frame_active(unit) {
        return None;
    }

    let angle = usize::from(body_angle % 8);

    if matches!(unit.unit_type, UnitType::Infiltrator | UnitType::Infantry) {
        // `unit_angle + 104`, then +8 for each recoil tick (8 РІвЂ вЂ™ 0).
        let step = 8usize.saturating_sub(usize::from(unit.firing_recoil_frames));
        return Some(104 + angle + 8 * step);
    }

    if unit.flags & unit_flags::TURRET_SPRITE != 0 {
        return None;
    }

    if info.firing_image_count == 0 {
        return None;
    }

    Some(usize::from(info.firing_image_base) + angle)
}

/// **Turret** frame during recoil (`UnitInfo::PrepareFire`/`ProgressFire`).
///
/// C++ `DrawSpriteTurretFrame(turret_angle + firing_image_base)`, and for
/// `ANTIAIR`/`SP_FLAK`/`FASTBOAT` each `ProgressFire` additionally
/// toggles `turret_image_index Р’В± 8` (`unitinfo.cpp:4828-4830`), so the
/// flash alternates with the normal turret frame. The sign of the first toggle
/// depends on the parity of the initial counter (15 for `ANTIAIR`, 6 for
/// `SP_FLAK`/`FASTBOAT`) РІР‚вЂќ here it is computed from `firing_recoil_frames`.
///
/// `None` РІР‚вЂќ the unit has no turret/firing frames, the turret is drawn with the normal frame.
pub fn firing_turret_frame_index(
    info: &max_assets::sprite::FrameInfo,
    unit: &max_core::unit::UnitInfo,
) -> Option<usize> {
    if !firing_frame_active(unit) || unit.flags & unit_flags::TURRET_SPRITE == 0 {
        return None;
    }

    if info.firing_image_count == 0 {
        return None;
    }

    let base = i32::from(info.firing_image_base);
    let angle = i32::from(unit.turret_angle % 8);
    let recoil = i32::from(unit.firing_recoil_frames);

    let special = matches!(
        unit.unit_type,
        UnitType::AntiAir | UnitType::MobileAntiAir | UnitType::Escort
    );

    let shift = if special {
        // Initial `PrepareFire` counter: 3 Р“вЂ” 5 for ANTIAIR, 3 Р“вЂ” 2 for the others.
        let initial = if unit.unit_type == UnitType::AntiAir {
            15
        } else {
            6
        };

        if initial % 2 == 1 {
            // ANTIAIR: 15 РІвЂ вЂ™ 14 (even) toggles to firing+8.
            if recoil % 2 == 0 {
                8
            } else {
                0
            }
        } else if recoil % 2 == 0 {
            0
        } else {
            -8
        }
    } else {
        0
    };

    let index = base + angle + shift;

    (index >= 0).then_some(index as usize)
}

/// Pixel anchor of a flying projectile (`MISSILE_UNIT`).
///
/// C++ `AirPath::Execute` moves the projectile **pixel by pixel**
/// (`unit->OffsetDrawZones`, `airpath.cpp:219-242`), and `UnitInfo::Render` draws
/// it exactly at `unit->x/y`. The port keeps the same pixel position in
/// `UnitInfo::x/y` (`unit_order::missile_path_execute`), so the projectile flies
/// in a straight line instead of jumping by cell centers.
pub fn missile_pixel_anchor(unit: &max_core::unit::UnitInfo) -> Option<(i32, i32)> {
    (unit.flags & unit_flags::MISSILE_UNIT != 0).then(|| (i32::from(unit.x), i32::from(unit.y)))
}

/// Pixel anchor of a trail effect unit (rocket smoke/torpedo bubbles).
///
/// C++ `Paths_DrawMissile` (`airpath.cpp:392-404`) puts three particles into
/// the vacated cell and spreads the **draw zones** over 0/РІвЂ¦вЂњ/РІвЂ¦вЂќ of the segment
/// (`OffsetDrawZones`); `unit->x/y` remain the cell center. The port
/// keeps the spread in `x/y` (see `unit_order::spawn_missile_trail`), so
/// the effect renderer takes them directly.
pub fn effect_pixel_anchor(unit: &max_core::unit::UnitInfo) -> Option<(i32, i32)> {
    matches!(
        unit.unit_type,
        UnitType::RocketSmoke | UnitType::TorpedoBubble
    )
    .then(|| (i32::from(unit.x), i32::from(unit.y)))
}

/// Pixel anchor of an explosion effect (`UnitsManager_StartExplosion`,
/// `units_manager.cpp:3991-3994`).
///
/// C++ deploys the effect on the dying unit's cell and then
/// `explosion->OffsetDrawZones(unit->x - explosion->x, unit->y - explosion->y)`:
/// the blast is drawn at the **unit's own pixel**, not at the cell anchor. The
/// port stores the result in the effect's `x/y`, so a unit shot down between two
/// cells (an aircraft on its air path) explodes exactly where it was, and a unit
/// stopped by an intercept explodes on the halt cell's center.
pub fn explosion_pixel_anchor(unit: &max_core::unit::UnitInfo) -> Option<(i32, i32)> {
    (unit.flags & unit_flags::EXPLODING != 0).then(|| (i32::from(unit.x), i32::from(unit.y)))
}

/// Pixel anchor of a flying aircraft.
///
/// `AirPath::Execute` (`airpath.cpp:219-242`) moves the aircraft **pixel by pixel**
/// along the straight segment (`m_step_x += m_delta_x`, `OffsetDrawZones`), and
/// `UnitInfo::Render` draws it exactly at `unit->x/y`. The port keeps the straight
/// line in the simulation (`unit_order::air_path_execute`) and interpolates the
/// frame-side point in cells (`App::update_visual_units`): the anchor is that point
/// in world pixels, 64 px per cell — no cell-center offset, unlike
/// [`unit_anchor_world`].
pub fn air_pixel_anchor(unit: &max_core::unit::UnitInfo, visual: (f32, f32)) -> Option<(i32, i32)> {
    (unit.is_air() && unit.flags & unit_flags::MISSILE_UNIT == 0).then(|| {
        (
            (visual.0 * max_core::unit_order::GFX_MAP_TILE_SIZE as f32).round() as i32,
            (visual.1 * max_core::unit_order::GFX_MAP_TILE_SIZE as f32).round() as i32,
        )
    })
}

/// Unit frame taking the builder frame-by-frame animation into account (`build_step`).
///
/// `build_step` РІР‚вЂќ number of the constructor working cycle frame (0, 1, 2) from
/// [`crate::App::build_animation_step`]; `0` РІР‚вЂќ the construction start frame `+16`, as
/// `DrawSpriteFrame(image_index + 16)` in C++. Then the C++ cycle repeats
/// `UnitsManager_ProcessOrderBuild` (`units_manager.cpp:2856`):
///
/// ```text
/// index = index + 8;                    // DrawSpriteFrame(image_index + 8)
/// if index + 8 >= 40 { index -= 16; }   // frames 40+ do not fit into CONSTRCT
/// ```
///
/// The exact C++ formula is applied to the current index (not to `angle+16+8n`),
/// so the sequence wraps exactly as in the original:
/// `16 РІвЂ вЂ™ 24 РІвЂ вЂ™ 32 РІвЂ вЂ™ 16` (for angle 0). `ENGINEER` has no cycle РІР‚вЂќ frame `+16`
/// is constant, like `DrawSpriteFrame(image_index + 16)` in `StartBuilding`.
pub fn unit_frame_index_for_animation(
    unit: &max_core::unit::UnitInfo,
    image_base: i16,
    image_count: usize,
    angle: u8,
    build_step: u8,
) -> usize {
    unit_frame_index_with_animation(
        unit,
        image_base,
        image_count,
        image_count,
        angle,
        FrameAnimation {
            builder_step: build_step,
            ..FrameAnimation::default()
        },
        true,
    )
}

/// Unit frame with all C++ frame-by-frame animations (builder and `ANIMATED`).
///
/// `image_count` РІР‚вЂќ the number of body sprite frames (the index bound);
/// `sentry_frames` РІР‚вЂќ `Unit::GetFrameInfo().image_count` (`D_*`), i.e.
/// how many frames the animation block occupies. `RADAR` has 20 frames in the sprite, but
/// `D_RADAR.image_count = 16` (the last 4 are connectors), and C++
/// `image_index_max` counts by `FrameInfo` РІР‚вЂќ so the animation must
/// wrap at 16, not at 20.
///
/// Order as in C++:
///
/// 1. static frame [`unit_frame_index_for_angle`] (`image_base + angle`,
///    `+1` at `POWER_ON` and for a working factory, `+16` the builder working
///    pose);
/// 2. the constructor cycle `+8` / `-16` ([`builder_animation_active`]);
/// 3. `ProcessOrderSentry` for `ANIMATED` (`sentry_step`, reset to
///    image_base after the last frame).
///
/// `effects = false` disables item 3, like `settings["effects"]` in C++.
///
/// **Building frames are not advanced by time** (DOS capture Р’В§7.2,
/// `max-original-dasm/formats/unit-animation.md`): factories have two state
/// frames, the visible blinking of a powered-on factory РІР‚вЂќ palette cycles
/// (`World::InitColorCycles`), not the sprite. Therefore there is no factory frame
/// counter here: `dt = 0` and `dt > 0` give the same body frame, and only
/// the palette blinks ([`max_render::palette::ColorCycles`]).
#[allow(clippy::too_many_arguments)]
pub fn unit_frame_index_with_animation(
    unit: &max_core::unit::UnitInfo,
    image_base: i16,
    image_count: usize,
    sentry_frames: usize,
    angle: u8,
    animation: FrameAnimation,
    effects: bool,
) -> usize {
    let last = image_count.saturating_sub(1);

    // `UnitsManager_ProgressExplosion` (`units_manager.cpp:4006`) moves
    // the effect unit's `image_index`, and `UnitInfo::Render` draws exactly
    // `GetImageIndex()`. Explosions have no phases/rotations РІР‚вЂќ the frame is taken directly.
    if unit.flags & unit_flags::EXPLODING != 0 {
        return usize::try_from(unit.image_index.max(0))
            .unwrap_or(0)
            .min(last);
    }

    let mut index = unit_frame_index_for_angle(unit, image_base, image_count, angle);

    if builder_animation_active(unit) {
        for _ in 0..animation.builder_step % BUILDER_WORK_FRAME_STEPS {
            index = if index + BUILDER_WORK_FRAME_STEP >= image_count {
                index.saturating_sub(BUILDER_WORK_FRAME_OFFSET)
            } else {
                index + BUILDER_WORK_FRAME_STEP
            };
        }
    }

    if effects && sentry_animation_active(unit) {
        let base = usize::try_from(image_base.max(0)).unwrap_or(0);
        let frames = sentry_frames.max(1);

        // C++ `ProcessOrderSentry` (`units_manager.cpp:3029-3041`): start РІР‚вЂќ
        // `image_base + angle` (from the `UnitInfo` constructor), then `+1` until
        // `image_index_max`, after the last frame РІР‚вЂќ `image_base` again
        // (not `image_base + angle`). This is exactly
        // `base + ((angle + step) % image_count)`; for `RADAR` (angle = 0,
        // `D_RADAR.image_count = 16`) РІР‚вЂќ 16 rotation frames.
        let phase = (usize::from(angle % 8) + animation.sentry_step as usize) % frames;
        index = base + phase;
    }

    index.min(last)
}

/// World point of the sprite anchor: the center of the visual cell, for buildings РІР‚вЂќ +31.
///
/// C++ `UnitInfo::SetPosition` (`unitinfo.cpp:1301`) and `Render`
/// (`unitinfo.cpp:3138`) draw at `unit.x/unit.y`; for `BUILDING` this is the center of
/// the 2Р“вЂ”2 rectangle, for the others РІР‚вЂќ the cell center.
pub fn unit_anchor_world(unit: &max_core::unit::UnitInfo, visual: (f32, f32)) -> (i32, i32) {
    let mut x = (visual.0 * 64.0 + 32.0).round() as i32;
    let mut y = (visual.1 * 64.0 + 32.0).round() as i32;

    if unit.is_quad() {
        x += BUILDING_ANCHOR_OFFSET;
        y += BUILDING_ANCHOR_OFFSET;
    }

    (x, y)
}

/// Diagnostics of one frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderStats {
    /// How many map cells were drawn.
    pub map_tiles: u32,
    /// How many resource marker pixels were drawn ([`Self::resource_icon_cells`]
    /// РІР‚вЂќ how many cells).
    pub resource_icons: u32,
    /// How many survey markers were drawn (`Survey_RenderMarkers`,
    /// `survey.cpp:123`): every **surveyed** cell of the view gets one РІР‚вЂќ `*MK1..16`
    /// for a known deposit, `*MK0` for an explored cell without a resource.
    pub resource_icon_cells: u32,
    /// How many units were drawn with sprites.
    pub units_drawn: usize,
    /// How many units were drawn with markers (no sprite/assets).
    pub units_marker: usize,
    /// How many `BLDMRK*` construction markers were drawn (`DrawMap_RenderBuildMarker`).
    pub build_markers: usize,
    /// How many route arrows of the selected unit were drawn
    /// (`GameManager_RenderScanRangeIndicators` в†’ `Paths_DrawMarker`).
    pub route_markers: usize,
    /// How many selection indicators were drawn (`GameManager_DrawUnitSelector`).
    pub selector_units: usize,
    /// How many units were skipped (not in the lists/not visible to the camera).
    pub units_skipped: usize,
    /// How many pixels of the 3Р“вЂ”3 button overlays (GRID/HITS/AMMO/COLORS/STATUS/
    /// NAMES/RANGE/SCAN) were drawn РІР‚вЂќ [`crate::overlays`].
    pub overlay_pixels: u32,
}

impl RenderStats {
    /// Total drawn units.
    pub fn units(self) -> usize {
        self.units_drawn + self.units_marker
    }
}

/// What was drawn for one unit (diagnostics and type coverage tests).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UnitSpriteStats {
    /// Body pixels (together with the shadow РІР‚вЂќ it only darkens what is already
    /// drawn, so on an empty frame this is exactly the body).
    pub unit: u32,
    /// Turret/gun pixels; 0 РІР‚вЂќ the type has no turret or flag.
    pub turret: u32,
    /// Connector pixels (`UnitInfo::RenderWithConnectors`): for a mine РІР‚вЂќ 8
    /// frames of `D_MINING.connector_image_base = 16`, for `POWGEN` РІР‚вЂќ 4 pipes
    /// `D_SMLBLD.connector_image_base = 2`. 0 РІР‚вЂќ no connectors/no neighbors.
    pub connectors: u32,
}

/// Draws a unit with sprites: shadow РІвЂ вЂ™ body РІвЂ вЂ™ connectors РІвЂ вЂ™ turret/gun.
///
/// Repeats C++ `UnitInfo::RenderWithConnectors`/`Render`: body with
/// `zoom/64`, shadow with the `shadow_offset` shift, complex connectors as separate
/// frames `connector_image_base + n`, turret as a separate frame
/// `turret_image_base + turret_angle` with the `angle_offsets[angle]` shift.
///
/// The frame base is `unit.image_base` (without the mine clan variant). The map and HUD
/// call [`draw_unit_sprites_with_base`] with
/// `image_base_for(unit, clan)`, so the mine shows its clan.
///
/// Returns `None` if there is no loaded sprite/frame for the type РІР‚вЂќ then the
/// caller draws the team marker (the only such type in the game data is
/// `MASTER`, see [`crate::assets::UNIT_TYPES_WITHOUT_SPRITE_DATA`]).
///
/// This is the common point of the map ([`App::draw_units`]), the HUD portrait
/// (`panels::draw_portrait`) and the tests that run the renderer over all 93
/// unit types.
pub fn draw_unit_sprites(
    assets: &crate::assets::Assets,
    unit: &max_core::unit::UnitInfo,
    presentation: &UnitPresentation,
    frame: &mut RgbaBuffer,
    anchor: (i32, i32),
    zoom: u32,
) -> Option<UnitSpriteStats> {
    draw_unit_sprites_with_base(
        assets,
        unit,
        presentation,
        unit.image_base,
        frame,
        anchor,
        zoom,
    )
}

/// [`draw_unit_sprites`] with an explicit body frame base (clan/save/degree of
/// building power-on). The frame is chosen by [`unit_frame_index`].
pub fn draw_unit_sprites_with_base(
    assets: &crate::assets::Assets,
    unit: &max_core::unit::UnitInfo,
    presentation: &UnitPresentation,
    image_base: i16,
    frame: &mut RgbaBuffer,
    anchor: (i32, i32),
    zoom: u32,
) -> Option<UnitSpriteStats> {
    draw_unit_sprites_with_angle(
        assets,
        unit,
        presentation,
        image_base,
        unit.angle,
        frame,
        anchor,
        zoom,
    )
}

/// [`draw_unit_sprites_with_base`] with an explicit body angle for the frame.
///
/// `body_angle` РІР‚вЂќ the direction with which the sprite **currently** moves on the map
/// (in the window loop this is `App::visual_angle`). The body takes the frame
/// `image_base + body_angle`, and the turret РІР‚вЂќ its own `turret_image_base +
/// turret_angle` with the `angle_offsets[body_angle]` shift, as in
/// `UnitInfo::RenderWithConnectors` (`unitinfo.cpp:4838`). The other
/// callers (HUD portrait, tests) pass `unit.angle`.
pub fn draw_unit_sprites_with_angle(
    assets: &crate::assets::Assets,
    unit: &max_core::unit::UnitInfo,
    presentation: &UnitPresentation,
    image_base: i16,
    body_angle: u8,
    frame: &mut RgbaBuffer,
    anchor: (i32, i32),
    zoom: u32,
) -> Option<UnitSpriteStats> {
    draw_unit_sprites_with_animation(
        assets,
        &assets.system_palette,
        unit,
        presentation,
        image_base,
        body_angle,
        FrameAnimation::default(),
        true,
        unit.connectors,
        frame,
        anchor,
        zoom,
    )
}

/// `UnitInfo::ShakeAir` (`unitinfo.cpp:4636-4678`) РІР‚вЂќ the hovering flight bob.
///
/// C++ calls `Animate()` once per order tick for airborne units (`HOVERING`);
/// `ShakeAir` advances the 6-bit `moved` counter and, on every **even** tick,
/// applies `OffsetDrawZones(Р’В±DIRECTION_OFFSETS[moved / 8])`:
///
/// ```text
/// distance = moved / 8;                       // 0..7, one direction per 8 ticks
/// if (moved >= distance * 8 + 4)   offset = -DIRECTION_OFFSETS[distance];
/// else                             offset = +DIRECTION_OFFSETS[distance];
/// ```
///
/// The offsets add up inside an 8-tick block (two even ticks per half) and cancel
/// out at the end of the block: the hull swings at most **2 px** along the current
/// direction. The shadow is shifted on top of the hull by `shadow_offset -= 2 *
/// offset` (`unitinfo.cpp:4666-4667`), i.e. its absolute motion is three times
/// the hull's.
///
/// Returns the hull offset in map pixels for `moved` (`0..64`) РІР‚вЂќ the same
/// absolute offset that the cumulative `OffsetDrawZones` calls produce.
pub fn shake_air_offset(moved: u8) -> (i32, i32) {
    let distance = usize::from(moved / 8) % max_core::geo::DIRECTION_OFFSETS.len();
    let within_block = moved % 8;
    let direction = max_core::geo::DIRECTION_OFFSETS[distance];
    let mut dx = 0;
    let mut dy = 0;

    for step in 0..=u32::from(within_block) {
        if step & 1 != 0 {
            continue;
        }

        // The first half of the block pushes out, the second half pulls back.
        let sign = if step < 4 { 1 } else { -1 };

        dx += sign * i32::from(direction.x);
        dy += sign * i32::from(direction.y);
    }

    (dx, dy)
}

/// Presentation shadow offset of a bobbing hovering unit.
///
/// `UnitInfo::ShakeAir` (`unitinfo.cpp:4666-4667`) applies the hull's
/// `OffsetDrawZones(offset)` **and** `shadow_offset -= 2 * offset`; the hull
/// anchor is moved by `offset` by the renderer, so subtracting `2 * offset` from
/// the stored value places the shadow `3 * offset` from its base while the hull
/// moves `offset`. Sign convention: the stored value is negated by
/// [`shadow_render_offset`], hence the same `-=`.
pub fn bobbed_shadow_offset(presentation: &UnitPresentation, bob: (i32, i32)) -> Point {
    Point::new(
        presentation.shadow_offset.x - (2 * bob.0) as i16,
        presentation.shadow_offset.y - (2 * bob.1) as i16,
    )
}

/// Shadow shift handed to the sprite renderer РІР‚вЂќ `UnitInfo::RenderShadow`
/// (`unitinfo.cpp:3067`: `point -= shadow_offset`).
///
/// [`UnitPresentation::shadow_offset`] stores the C++ value verbatim (`(-64, -64)`
/// for a hovering aircraft, see `max_core::unit_order::air_shadow_offset`), while
/// [`SpriteRenderer::draw_unit_scaled`] places the shadow at `anchor + offset`;
/// hence the sign flip. A flying aircraft therefore gets its shadow one cell
/// down-right of the hull РІР‚вЂќ the original's "airborne" look that the port was
/// missing (defect 1 of the manual 1.2 run: aircraft drawn like parked units).
pub fn shadow_render_offset(presentation: &UnitPresentation) -> (i32, i32) {
    (
        -i32::from(presentation.shadow_offset.x),
        -i32::from(presentation.shadow_offset.y),
    )
}

/// [`draw_unit_sprites_with_angle`] with the unit frame clock.
///
/// `animation` РІР‚вЂќ [`FrameAnimation`] from [`crate::App::unit_animation`]:
/// the builder (`+16 РІвЂ вЂ™ +24 РІвЂ вЂ™ +32`), the `ANIMATED` building
/// (`ProcessOrderSentry`) and the `SPINNING_TURRET` radar
/// (`SpinningTurretAdvanceAnimation`: AWAC/SCANNER). Mines and factories have
/// **no** time-based frame cycle
/// (they hold the C++ state frame; the blinking is the palette). The HUD and tests
/// pass [`FrameAnimation::default()`] (the static C++ frame, as with `dt = 0`).
///
/// `effects` РІР‚вЂќ `settings["effects"]`; `false` disables frame-by-frame animations,
/// as in C++.
///
/// The palette РІР‚вЂќ the game system palette (`Assets::system_palette`); the map
/// passes a snapshot with palette cycles here (`App::animated_palette`),
/// so that the building beacons blink. Connectors are drawn by the `connectors` mask
/// (see [`connector_frame_indices`]).
#[allow(clippy::too_many_arguments)]
pub fn draw_unit_sprites_with_animation(
    assets: &crate::assets::Assets,
    palette: &max_render::palette::Palette,
    unit: &max_core::unit::UnitInfo,
    presentation: &UnitPresentation,
    image_base: i16,
    body_angle: u8,
    animation: FrameAnimation,
    effects: bool,
    connectors: u16,
    frame: &mut RgbaBuffer,
    anchor: (i32, i32),
    zoom: u32,
) -> Option<UnitSpriteStats> {
    let sprite = assets.sprite(unit.unit_type)?;
    // `UnitInfo::SpotByTeam` surfaces a spotted submarine / personnel carrier:
    // `image_base = 8` (`unitinfo.cpp:2825-2829`), i.e. the **second** block of
    // eight hull frames of the same `D_*`/`S_*` resource; the simulation field is
    // `0` while the boat is submerged. `SUBMARNE`/`CLNTRANS` sprites have exactly
    // 16 frames, so the base is used verbatim РІР‚вЂќ adding another 8 addressed frames
    // 16..23 that do not exist and the surfaced unit vanished from the frame.
    // The frame inside the block is still derived from the order state.
    // `FrameInfo` (`D_*`) defines the length of the animation frame block; for RADAR it is 16,
    // although the sprite has 20 frames (the last 4 are connectors).
    let sentry_frames = sprite
        .frame_info
        .map(|info| usize::from(info.image_count))
        .filter(|frames| *frames > 0)
        .unwrap_or(sprite.sheet.len());
    let image_index = {
        let mut index = unit_frame_index_with_animation(
            unit,
            image_base,
            sprite.sheet.len(),
            sentry_frames,
            body_angle,
            animation,
            effects,
        );

        // Shot flash (`UnitInfo::PrepareFire`/`ProgressFire`): the frame
        // `firing_image_base` (or 104+РІР‚В¦ for infantry) is held for the whole recoil.
        if let Some(info) = sprite.frame_info.as_ref() {
            if let Some(fire) = firing_body_frame_index(info, unit, body_angle) {
                index = fire.min(sprite.sheet.len().saturating_sub(1));
            }
        }

        index
    };
    let sprite_frame = sprite.frame_at(image_index)?;
    let shadow = sprite.shadow_at(image_index);
    let colors = TeamColors::ALL
        .get(unit.team.0 as usize)
        .copied()
        .unwrap_or(TeamColors::Derelict);
    let level = presentation.brightness.min(FULL_BRIGHTNESS_LEVEL);
    let renderer = SpriteRenderer::new(
        palette,
        &assets.brightness,
        ColorLut::for_team(colors),
        level,
    );

    // `UnitInfo::RenderShadow` (`unitinfo.cpp:3067`): `point -= shadow_offset`.
    // `UnitPresentation::shadow_offset` keeps the C++ sign convention (a hovering
    // aircraft has `(-64, -64)`, see `max_core::unit_order::air_shadow_offset`), and
    // the sprite renderer places the shadow at `anchor + offset`, so the value is
    // negated here РІР‚вЂќ the shadow of a flying aircraft lands one cell **down-right**
    // of the hull, which is the original's "airborne" look.
    let shadow_offset = shadow_render_offset(presentation);
    let mut stats = UnitSpriteStats::default();

    stats.unit = renderer
        .draw_unit_scaled(frame, sprite_frame, shadow, anchor, shadow_offset, zoom)
        .drawn;

    // Complex connectors РІР‚вЂќ `UnitInfo::RenderWithConnectors`
    // (`unitinfo.cpp:3157`): each has its own frame `connector_image_base + n`,
    // drawn at the same anchor as the body (the frame hotspot defines
    // the side), the shadow РІР‚вЂќ from the same `S_*` as the body.
    stats.connectors = 0;

    for index in connector_frame_indices(sprite, connectors) {
        if let Some(connector) = sprite.sheet.frame(index) {
            stats.connectors += renderer
                .draw_unit_scaled(
                    frame,
                    connector,
                    sprite.shadow_at(index),
                    anchor,
                    shadow_offset,
                    zoom,
                )
                .drawn;
        }
    }

    // Turret/gun РІР‚вЂќ a separate frame over the body with the shift
    // `angle_offsets[body_angle]` (`UnitInfo::RenderWithConnectors`).
    if unit.flags & (unit_flags::TURRET_SPRITE | unit_flags::SPINNING_TURRET) != 0 {
        // During recoil the turret is drawn with the firing frame
        // (`DrawSpriteTurretFrame(turret_angle + firing_image_base)`).
        let firing_index = sprite
            .frame_info
            .as_ref()
            .and_then(|info| firing_turret_frame_index(info, unit))
            .filter(|index| *index < sprite.sheet.len());

        // `SPINNING_TURRET` РІР‚вЂќ the rotating radar (`AWAC`/`SCANNER`):
        // `UnitInfo::SpinningTurretAdvanceAnimation` (`unitinfo.cpp:3891`) walks
        // `turret_image_index` through the `turret_image_count` frames of `D_*`.
        // The stored C++ frame is `turret_image_base + turret_angle`; the port
        // keeps it in the simulation, the rotation itself is a frame-only clock
        // (`FrameAnimation::turret_step`), so the snapshot/CRC do not change.
        let spinning_index = if effects && spinning_turret_animation_active(unit) {
            sprite
                .frame_info
                .as_ref()
                .and_then(|info| spinning_turret_frame_index(info, unit, animation))
                .filter(|index| *index < sprite.sheet.len())
        } else {
            None
        };

        // The firing frame wins over the rotation (C++ draws only one turret
        // frame; `AWAC`/`SCANNER` have no firing frames at all, so this only
        // matters for a hypothetical type with both flags).
        let turret_index = firing_index.or(spinning_index);
        let turret = match turret_index {
            Some(index) => sprite.sheet.frame(index),
            None => sprite.turret_frame(unit.turret_angle),
        };
        let turret_shadow = match turret_index {
            Some(index) => sprite.shadow_at(index),
            None => sprite.turret_shadow(unit.turret_angle),
        };

        if let Some(turret) = turret {
            stats.turret = renderer
                .draw_turret_scaled(
                    frame,
                    turret,
                    turret_shadow,
                    anchor,
                    shadow_offset,
                    sprite.turret_offset(body_angle),
                    zoom,
                )
                .drawn;
        }
    }

    Some(stats)
}

/// The HD counterpart of [`draw_unit_sprites_with_animation`]
/// (`max-brh-docs/development/remaster-hd.md` §R4/§R7).
///
/// The frame indices, the team colour, the brightness level and the palette
/// cycle frame are resolved exactly like the 1:1 path ([`unit_frame_index_with_animation`],
/// the firing flash, [`connector_frame_indices`], the turret frames and the
/// `angle_offsets` shift). The drawing itself goes through
/// [`max_render::hd_sprites::HdSpriteRenderer`]: the body, the connector frames
/// and the turret use the HD index map of their frame when the remaster has one
/// and the original [`SpriteRenderer`] geometry otherwise, so a partial remaster
/// still renders.
///
/// At `zoom = 64·S` (the HD camera) an asset authored `×S` covers exactly the
/// `×S` nearest upscale of the 1:1 sprite, which is what makes the HD frame and
/// the upscaled 1:1 frame agree when every asset is an exact upscale.
///
/// `palette` is the frame palette (the animated snapshot or the system palette),
/// like in the 1:1 path; the palette cycle frame is passed separately because
/// the HD recolor LUT is keyed by it.
#[allow(clippy::too_many_arguments)]
pub fn draw_unit_sprites_with_animation_hd(
    assets: &crate::assets::Assets,
    palette: &max_render::palette::Palette,
    unit: &max_core::unit::UnitInfo,
    presentation: &UnitPresentation,
    image_base: i16,
    body_angle: u8,
    animation: FrameAnimation,
    effects: bool,
    connectors: u16,
    frame: &mut RgbaBuffer,
    anchor: (i32, i32),
    zoom: u32,
    hd: &mut HdPresenter,
    cycles: &max_render::palette::ColorCycles,
) -> Option<UnitSpriteStats> {
    let sprite = assets.sprite(unit.unit_type)?;
    // `D_*` `FrameInfo` bounds the animation block (RADAR: 16 of 20 frames).
    let sentry_frames = sprite
        .frame_info
        .map(|info| usize::from(info.image_count))
        .filter(|frames| *frames > 0)
        .unwrap_or(sprite.sheet.len());
    let mut image_index = unit_frame_index_with_animation(
        unit,
        image_base,
        sprite.sheet.len(),
        sentry_frames,
        body_angle,
        animation,
        effects,
    );

    // Shot flash (`UnitInfo::PrepareFire`/`ProgressFire`), as in the 1:1 path.
    if let Some(info) = sprite.frame_info.as_ref() {
        if let Some(fire) = firing_body_frame_index(info, unit, body_angle) {
            image_index = fire.min(sprite.sheet.len().saturating_sub(1));
        }
    }

    // Without a source frame there is nothing to draw in either path.
    sprite.frame_at(image_index)?;

    let colors = TeamColors::ALL
        .get(unit.team.0 as usize)
        .copied()
        .unwrap_or(TeamColors::Derelict);
    let level = presentation.brightness.min(FULL_BRIGHTNESS_LEVEL);
    let state = max_render::hd_sprites::HdRecolorState::new(colors, level).with_cycles(cycles);
    let (renderer, cache) =
        hd.sprite_render_context(unit.unit_type, palette, &assets.brightness, state)?;

    let shadow_offset = shadow_render_offset(presentation);
    let mut stats = UnitSpriteStats::default();

    stats.unit = renderer
        .draw_unit_scaled(frame, cache, image_index, anchor, shadow_offset, zoom)
        .drawn;

    for index in connector_frame_indices(sprite, connectors) {
        stats.connectors += renderer
            .draw_unit_scaled(frame, cache, index, anchor, shadow_offset, zoom)
            .drawn;
    }

    if unit.flags & (unit_flags::TURRET_SPRITE | unit_flags::SPINNING_TURRET) != 0 {
        let firing_index = sprite
            .frame_info
            .as_ref()
            .and_then(|info| firing_turret_frame_index(info, unit))
            .filter(|index| *index < sprite.sheet.len());
        let spinning_index = if effects && spinning_turret_animation_active(unit) {
            sprite
                .frame_info
                .as_ref()
                .and_then(|info| spinning_turret_frame_index(info, unit, animation))
                .filter(|index| *index < sprite.sheet.len())
        } else {
            None
        };
        // The firing frame wins over the rotation; otherwise the stored
        // `turret_image_base + turret_angle` frame of `D_*` (the same index
        // `LoadedSprite::turret_frame` uses).
        let turret_index = firing_index.or(spinning_index).or_else(|| {
            sprite
                .frame_info
                .as_ref()
                .filter(|info| info.turret_image_count > 0)
                .map(|info| info.turret_frame_index(unit.turret_angle))
        });

        if let Some(index) = turret_index {
            // `angle_offsets[body_angle]`, scaled in source-sprite pixels like
            // `SpriteRenderer::draw_turret_scaled`.
            let offset =
                max_render::hd_sprites::scaled_offset(sprite.turret_offset(body_angle), zoom);

            stats.turret = renderer
                .draw_unit_scaled(
                    frame,
                    cache,
                    index,
                    (anchor.0 + offset.0, anchor.1 + offset.1),
                    shadow_offset,
                    zoom,
                )
                .drawn;
        }
    }

    Some(stats)
}

/// Team marker colors (fallback without sprites): the original team order РІР‚вЂќ
/// `RED, GREEN, BLUE, GRAY, ALIEN`.
pub const MARKER_COLORS: [Rgba; 5] = [
    Rgba::rgb(224, 48, 48),
    Rgba::rgb(48, 192, 72),
    Rgba::rgb(64, 112, 224),
    Rgba::rgb(192, 192, 192),
    Rgba::rgb(212, 168, 64),
];

/// Blink period of the selection indicator (`GameManager_MarkerColor`).
///
/// C++ (`game_manager.cpp:1189-1192`): `GameManager_MarkerColor ^= 0xFF` every
/// `TIMER_FPS_TO_MS(24) * 5` = 42 ms Р“вЂ” 5 = **210 ms**. The color is the palette index
/// `0xFF` (white) or `0x00` (black); `DrawUnitSelector` draws three nested
/// frames with alternation (`color ^= 0xFF` after each).
pub const UNIT_SELECTOR_BLINK_SECONDS: f32 = 0.210;

/// Palette for the selection/construction markers without assets: index `0xFF` РІР‚вЂќ white,
/// `0x00` РІР‚вЂќ black.
fn fallback_selector_palette() -> PaletteRgba {
    let mut palette = [[0u8, 0, 0, 255]; 256];
    palette[0xFF] = [255, 255, 255, 255];
    palette
}

/// Pixel segment with clipping by the buffer (only horizontal or
/// vertical РІР‚вЂќ there are no other `draw_line` in `GameManager_DrawUnitSelector`).
fn selector_line(frame: &mut RgbaBuffer, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgba) -> u32 {
    let mut painted = 0;

    if y0 == y1 {
        let y = y0;
        for x in x0.min(x1)..=x0.max(x1) {
            if frame.set(x, y, color) {
                painted += 1;
            }
        }
    } else if x0 == x1 {
        let x = x0;
        for y in y0.min(y1)..=y0.max(y1) {
            if frame.set(x, y, color) {
                painted += 1;
            }
        }
    }

    painted
}

/// Selected unit indicator РІР‚вЂќ `GameManager_DrawUnitSelector`
/// (`game_manager.cpp:744-836`), called from `DrawMap_RenderUnit`
/// (`drawmap.cpp:813-833`).
///
/// `anchor` РІР‚вЂќ the screen center of the sprite (`UnitInfo::x/y`, i.e. the cell center,
/// for `BUILDING` РІР‚вЂќ `+31`, for the builder РІР‚вЂќ with the tape shift). `is_building`
/// selects the corner length: C++ `size1` = 32 for a building and 16 for the others, the frame
/// step `size2` = 128/64, the anchor "half-size" РІР‚вЂќ 64/32; everything is converted to
/// screen pixels as `value * 64 / Gfx_MapScalingFactor` =
/// `value * zoom / 64`.
///
/// The map always passes `double_marker = true`, so **three** nested
/// frames are drawn: color `marker_color`, `marker_color ^ 0xFF`,
/// `marker_color` (the inner frame is 1 px closer and 2 px shorter).
/// Returns the number of drawn pixels.
pub fn draw_unit_selector(
    frame: &mut RgbaBuffer,
    palette: &PaletteRgba,
    anchor: (i32, i32),
    is_building: bool,
    zoom: u32,
    marker_color: u8,
) -> u32 {
    let zoom = zoom.max(1) as i32;
    let (size1_world, size2_world, half_world) = if is_building {
        (32, 128, 64)
    } else {
        (16, 64, 32)
    };

    let mut size1 = size1_world * zoom / 64;
    let mut size2 = size2_world * zoom / 64 - 2;
    let half = half_world * zoom / 64;
    let mut offsetx = anchor.0 - half + 1;
    let mut offsety = anchor.1 - half + 1;
    let mut color = marker_color;
    let mut painted = 0;

    for _ in 0..3 {
        let rgba = Rgba::from_rgba(palette[usize::from(color)]);
        let x0 = offsetx;
        let y0 = offsety;

        // Top-left.
        painted += selector_line(frame, x0, y0, x0 + size1, y0, rgba);
        painted += selector_line(frame, x0, y0, x0, y0 + size1, rgba);

        let x1 = x0 + size2;

        // Top-right.
        painted += selector_line(frame, x1 - size1, y0, x1, y0, rgba);
        painted += selector_line(frame, x1, y0, x1, y0 + size1, rgba);

        let y1 = y0 + size2;

        // Bottom-right.
        painted += selector_line(frame, x1 - size1, y1, x1, y1, rgba);
        painted += selector_line(frame, x1, y1, x1, y1 - size1, rgba);

        // Bottom-left.
        painted += selector_line(frame, x0, y1, x0 + size1, y1, rgba);
        painted += selector_line(frame, x0, y1, x0, y1 - size1, rgba);

        size2 -= 2;
        offsetx += 1;
        offsety += 1;
        size1 -= 1;
        color ^= 0xFF;
    }

    painted
}

/// A 1:1 UI layer composited over the HD frame ([`App::render_frame_hd`]).
///
/// The remaster keeps the 640×480 layout for the interface (`remaster-hd.md`
/// §R5): these layers are drawn by the historical code and enter the HD frame
/// as the nearest `×S` upscale, the per-asset fallback of the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Foreground {
    /// The in-game panels (`Panels::draw`): buttons, counters, minimap.
    Panels,
    /// The modal windows and dialogs (`App::draw_modals`).
    Modals,
}

/// Copies `source` into `frame` at `(ulx, uly)` with clipping.
fn blit_at(frame: &mut RgbaBuffer, source: &RgbaBuffer, ulx: i32, uly: i32) -> u32 {
    let mut painted = 0;

    for y in 0..source.height() as i32 {
        for x in 0..source.width() as i32 {
            let Some(color) = source.get(x, y) else {
                continue;
            };

            if frame.set(ulx + x, uly + y, color) {
                painted += 1;
            }
        }
    }

    painted
}

/// Composites a 640×480 UI layer over an HD frame as the nearest `×S` upscale.
///
/// Only pixels the layer actually painted are copied (`A = 0` is "untouched"):
/// the layer was drawn into an empty buffer, so the HD frame — chrome, map,
/// sprites — stays visible everywhere the UI did not draw. That is the
/// per-asset fallback: an interface asset without an HD counterpart is the
/// `×S` upscale of its 1:1 picture.
pub(crate) fn composite_upscaled(frame: &mut RgbaBuffer, layer: &RgbaBuffer, factor: u32) {
    let factor = factor.max(1) as i32;

    for y in 0..layer.height() as i32 {
        for x in 0..layer.width() as i32 {
            let Some(color) = layer.get(x, y) else {
                continue;
            };

            if color.is_transparent() {
                continue;
            }

            for dy in 0..factor {
                for dx in 0..factor {
                    frame.set(x * factor + dx, y * factor + dy, color);
                }
            }
        }
    }
}

impl<D: Dispatcher> App<D> {
    /// Draws a frame at the HD presentation scale of `hd`
    /// (`max-brh-docs/development/remaster-hd.md` §R7).
    ///
    /// Presentation only: the camera, the simulation and every UI coordinate
    /// stay in 640×480. The frame is `640·S × 480·S`:
    ///
    /// * the map is drawn with the `×S` camera ([`Viewport::scaled`]) — terrain
    ///   tiles from [`max_render::hd_terrain`] and unit sprites from
    ///   [`max_render::hd_sprites`] where the remaster has an asset, the
    ///   original data otherwise (per-asset fallback);
    /// * the window background is the HD `FRAMEPIC` when resolved, else the
    ///   original palette picture nearest-scaled `×S`;
    /// * the layers that are laid out in 640×480 (panels, dialogs) are drawn at
    ///   1:1 and composited as the nearest `×S` upscale;
    /// * the message box is laid out natively at `×S` with the HD `Font5`
    ///   atlas ([`max_render::hd_text`]).
    ///
    /// In [`RenderScale::PixelPerfect`] the presenter cannot be HD and this
    /// method returns the historical frame: `S = 1` stays byte-for-byte
    /// [`App::render_frame_with_stats`].
    ///
    /// Anti-aliasing is **not** part of the render frame
    /// (`max-brh-docs/development/gpu-aa-design.md`): `S` is the only render
    /// scale, and `aa` is applied by the output layer
    /// ([`max_render::GpuPresenter`]) after this frame is finished. Two frames
    /// of the same scene are therefore byte-identical whatever `aa` is — the
    /// snapshot/CRC and the golden frames cannot see the setting.
    pub fn render_frame_hd(&self, hd: &mut HdPresenter) -> RgbaBuffer {
        self.render_frame_hd_with_stats(hd).0
    }

    /// [`App::render_frame_hd`] plus drawing statistics.
    pub fn render_frame_hd_with_stats(&self, hd: &mut HdPresenter) -> (RgbaBuffer, RenderStats) {
        let scale = hd.scale();

        if !scale.is_hd() {
            return self.render_frame_with_stats();
        }

        // `S` is both the asset scale of the HD layers and the render factor of
        // this frame: `aa` never changes it (it belongs to the output layer).
        let factor = scale.scale();
        let (width, height) = self.viewport().screen();
        let style = TileStyle::default();
        let mut stats = RenderStats::default();
        let mut frame = RgbaBuffer::filled(width * factor, height * factor, style.unknown);

        if self.chrome_active() {
            // The HD window background (or the original palette `FRAMEPIC`
            // nearest-scaled to the render factor).
            hd.draw_chrome_frame_at(&mut frame, factor);

            let area = max_render::layout::MAIN_MAP;
            let scaled_area = scaled_pixel_rect_by(area, factor);
            let mut map = RgbaBuffer::filled(
                scaled_area.width.max(1) as u32,
                scaled_area.height.max(1) as u32,
                style.unknown,
            );
            let viewport = self.map_viewport().scaled(factor);

            self.draw_map_hd(&mut map, &viewport, &mut stats, hd, true);
            blit_at(&mut frame, &map, scaled_area.ulx, scaled_area.uly);

            // Interface layers, bottom to top: panels, the message box over
            // the map, then the modal windows.
            self.composite_foreground(&mut frame, factor, Foreground::Panels);
            self.draw_status_message_hd(&mut frame, scaled_area, hd.fonts());
            self.composite_foreground(&mut frame, factor, Foreground::Modals);
        } else {
            // Full-screen mode: the whole frame is the map.
            let viewport = self.viewport().scaled(factor);

            self.draw_map_hd(&mut frame, &viewport, &mut stats, hd, false);
            self.composite_foreground(&mut frame, factor, Foreground::Modals);
            let whole =
                scaled_pixel_rect_by(PixelRect::new(0, 0, width as i32, height as i32), factor);
            self.draw_status_message_hd(&mut frame, whole, hd.fonts());
        }

        (frame, stats)
    }

    /// The map of the HD frame, drawn with the `×S` camera.
    ///
    /// The draw order is the one of the 1:1 windowed renderer; every overlay
    /// takes its geometry from the camera ([`Viewport::tile_rect`],
    /// [`Viewport::world_to_screen`]), so passing the scaled camera places it at
    /// `×S` without touching the drawing code.
    fn draw_map_hd(
        &self,
        frame: &mut RgbaBuffer,
        viewport: &Viewport,
        stats: &mut RenderStats,
        hd: &mut HdPresenter,
        selected_path: bool,
    ) {
        let style = TileStyle::default();
        let has_hd_terrain = hd
            .terrain()
            .is_some_and(|terrain| terrain.hd_tile_count() > 0);

        if has_hd_terrain {
            if let Some(terrain) = hd.terrain() {
                stats.map_tiles = terrain.render_into(frame, viewport);
            }
        } else if let Some(assets) = self.assets().filter(|assets| !assets.terrain.is_empty()) {
            stats.map_tiles = assets.terrain.render_into(frame, viewport);
        } else {
            stats.map_tiles = render_map_into(frame, &self.state_ref().map(), viewport, &style);
        }

        self.draw_grid_overlay(frame, viewport, stats);
        self.draw_units(frame, viewport, stats, Some(hd));
        self.draw_toggle_overlays(frame, viewport, stats);
        self.draw_resource_icons(frame, viewport, stats);
        self.draw_build_markers(frame, viewport, stats);
        self.draw_build_site_ghost(frame, viewport, stats);

        if selected_path {
            self.draw_selected_unit_path(frame, viewport, stats);
        }
    }

    /// The modal windows over the map, in the original draw order.
    fn draw_modals(&self, frame: &mut RgbaBuffer) {
        self.draw_build_menu(frame);
        self.draw_save_load(frame);
        self.draw_save_confirm(frame);
        self.draw_upgrade_menu(frame);
        self.draw_research_menu(frame);
        self.draw_alloc_menu(frame);
        self.draw_transfer_menu(frame);
        self.draw_unit_menu(frame);
        self.draw_repair_shop(frame);
        self.draw_unit_stats(frame);
        self.draw_preferences(frame);
        self.draw_help(frame);
        self.draw_self_destruct(frame);
        self.draw_pause_dialog(frame);
    }

    /// Draws one 1:1 UI layer into an empty buffer and composites it over the
    /// HD frame as the nearest `×S` upscale.
    ///
    /// The layer is drawn by the historical code at 640×480; an untouched pixel
    /// stays transparent and keeps the HD frame visible. Drawings that read the
    /// destination back ([`crate::panels::draw_status_message`]) are not part of
    /// these layers — the message box is redrawn natively at `×S` by
    /// [`App::draw_status_message_hd`].
    fn composite_foreground(&self, frame: &mut RgbaBuffer, factor: u32, layer: Foreground) {
        let (width, height) = self.viewport().screen();
        let mut foreground = RgbaBuffer::filled(width, height, Rgba::TRANSPARENT);

        match layer {
            Foreground::Panels => {
                if let Some(panels) = self.assets().and_then(|assets| assets.panels.as_ref()) {
                    panels.draw(&mut foreground, self);
                }
            }
            Foreground::Modals => self.draw_modals(&mut foreground),
        }

        composite_upscaled(frame, &foreground, factor);
    }

    /// The HD [`App::draw_status_message`]: the message box laid out and drawn
    /// at the render scale with the HD `Font5` atlas (`R6`).
    ///
    /// With an HD atlas that is the exact `×S` upscale of the original bitmap
    /// this is byte-identical to the `×S` upscale of the 1:1 message box (the R6
    /// golden invariant): the metrics scale exactly, the box geometry is the
    /// `×S` of the logical one and the colour table is applied to the already
    /// upscaled pixels.
    fn draw_status_message_hd(
        &self,
        frame: &mut RgbaBuffer,
        area: PixelRect,
        fonts: &max_render::hd_text::HdFonts,
    ) {
        let Some(hint) = self.hud().hint.as_deref() else {
            return;
        };
        let Some(assets) = self.assets() else {
            return;
        };
        let palette = crate::panels::UiPalette::from_assets(assets)
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));
        let Some(palette) = palette else {
            return;
        };
        let snow = (self.config().planet as usize) < 6;

        crate::panels::draw_status_message_hd(
            frame,
            area,
            &palette,
            hint,
            self.hud().hint_style,
            snow,
            fonts.font(max_render::text::GnwFontId::Font5),
        );
    }

    /// Draws a frame of the current state into the `screen_width Р“вЂ” screen_height` buffer.
    pub fn render_frame(&self) -> RgbaBuffer {
        self.render_frame_with_stats().0
    }

    /// The same as [`App::render_frame`], plus drawing statistics.
    ///
    /// If the window chrome (`FRAMEPIC`) is loaded and the frame size is 640Р“вЂ”480, the
    /// 1:1 window is drawn as in the original: background panels from `MAX.RES`, and the map РІР‚вЂќ in the
    /// [`max_render::layout::MAIN_MAP`] area (7Р“вЂ”7 cells). Otherwise РІР‚вЂќ the former
    /// full-screen mode.
    pub fn render_frame_with_stats(&self) -> (RgbaBuffer, RenderStats) {
        let (width, height) = self.viewport().screen();

        if width == max_render::layout::WINDOW_WIDTH as u32
            && height == max_render::layout::WINDOW_HEIGHT as u32
        {
            if let Some(chrome) = self.assets().and_then(|assets| assets.chrome.as_ref()) {
                if chrome.is_ready() {
                    return self.render_with_chrome(chrome);
                }
            }
        }

        let style = TileStyle::default();
        let mut frame = RgbaBuffer::filled(width, height, style.unknown);
        let mut stats = RenderStats::default();

        // Surface: if the planet data is loaded РІР‚вЂќ real 64Р“вЂ”64 tiles from
        // `.WRL` (as in the original), otherwise the procedural `TileStyle` placeholder.
        match self.assets() {
            Some(assets) if !assets.terrain.is_empty() => {
                stats.map_tiles = assets.terrain.render_into(&mut frame, self.viewport());
            }
            _ => {
                stats.map_tiles =
                    render_map_into(&mut frame, &self.state_ref().map(), self.viewport(), &style);
            }
        }

        self.draw_grid_overlay(&mut frame, self.viewport(), &mut stats);
        self.draw_units(&mut frame, self.viewport(), &mut stats, None);
        // 3Р“вЂ”3 buttons of the left panel (`game_manager.cpp`): COLORS/STATUS/HITS/
        // AMMO/NAMES РІР‚вЂќ over the sprites, RANGE/SCAN РІР‚вЂќ circles over everything.
        self.draw_toggle_overlays(&mut frame, self.viewport(), &mut stats);
        // Resource icons РІР‚вЂќ over units/buildings (`GameManager_RenderSurveyIndicator`).
        self.draw_resource_icons(&mut frame, self.viewport(), &mut stats);
        // Construction markers РІР‚вЂќ last, like `DrawMap_RedrawDirtyZones`.
        self.draw_build_markers(&mut frame, self.viewport(), &mut stats);
        // Construction site ghost (`ORDER_STATE_SELECT_SITE`) РІР‚вЂќ over the units.
        self.draw_build_site_ghost(&mut frame, self.viewport(), &mut stats);

        // `BuildMenu` in full-screen mode (without `FRAMEPIC`).
        self.draw_build_menu(&mut frame);
        // Save/load window (P3) РІР‚вЂќ modal, like `AbstractMenu` in C++.
        self.draw_save_load(&mut frame);
        // Quick save/load `OKCancelMenu` (S1.3) РІР‚вЂќ over the game.
        self.draw_save_confirm(&mut frame);
        // Upgrades screen (P6).
        self.draw_upgrade_menu(&mut frame);
        // Research screen (P7) and the help window over it.
        self.draw_research_menu(&mut frame);
        // Mine extraction allocation dialog (`AllocMenu`, `ALLOCFRM`).
        self.draw_alloc_menu(&mut frame);
        // X-FER window (`TransferMenu`, `XFERPIC`) РІР‚вЂќ over the map.
        self.draw_transfer_menu(&mut frame);
        // Selected unit command menu (`GameManager_PopupButtons`).
        self.draw_unit_menu(&mut frame);
        // Unit statistics window (`UnitStats_Menu`) РІР‚вЂќ the full-screen `STATS`.
        self.draw_unit_stats(&mut frame);
        // `Preferences`/`Setup` dialog (`OptionsMenu`, `PREFSPIC`).
        self.draw_preferences(&mut frame);
        self.draw_help(&mut frame);
        // Building self-destruct dialog (UnitsManager_SelfDestructMenu).
        self.draw_self_destruct(&mut frame);
        // Pause dialog (PauseMenu_Menu -> DialogMenu, Alt+P) РІР‚вЂќ over everything.
        self.draw_pause_dialog(&mut frame);

        if self.hud().hint.is_some() {
            let whole = PixelRect::new(0, 0, frame.width() as i32, frame.height() as i32);
            self.draw_status_message(&mut frame, whole);
        }

        (frame, stats)
    }

    /// `MESSAGE_BOX_INFO` message window above the map РІР‚вЂќ `MessageManager_
    /// DrawMessage` (`message_manager.cpp:144`): the selected unit hint
    /// (`GameManager_DrawUnitStatusMessage`).
    fn draw_status_message(&self, frame: &mut RgbaBuffer, area: PixelRect) {
        let Some(hint) = self.hud().hint.as_deref() else {
            return;
        };
        let Some(assets) = self.assets() else {
            return;
        };
        let palette = crate::panels::UiPalette::from_assets(assets)
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));
        let Some(palette) = palette else {
            return;
        };

        // The snow tilesets are `SNOW_1..SNOW_6` (`World::GetMessageBoxNotice/
        // WarningTintParameters`, `world.cpp:1003-1026`).
        let snow = (self.config().planet as usize) < 6;

        crate::panels::draw_status_message(
            frame,
            area,
            &palette,
            hint,
            self.hud().hint_style,
            snow,
        );
    }

    /// Frame in the 1:1 window: `FRAMEPIC` + the map in the map area.
    fn render_with_chrome(&self, chrome: &crate::chrome::Chrome) -> (RgbaBuffer, RenderStats) {
        let style = TileStyle::default();
        let mut frame = RgbaBuffer::filled(
            max_render::layout::WINDOW_WIDTH as u32,
            max_render::layout::WINDOW_HEIGHT as u32,
            style.unknown,
        );
        chrome.draw(&mut frame);

        // The map is drawn into its own buffer the size of the map area: this way it
        // clips itself by the `MAIN_MAP` edges (as in the original), while the camera and zoom
        // remain shared with the application. `map_viewport` РІР‚вЂќ the same camera that
        // mouse input uses (`App::map_viewport`), otherwise the clicks drift.
        let area = max_render::layout::MAIN_MAP;
        let viewport = self.map_viewport();
        let mut map = RgbaBuffer::filled(area.width as u32, area.height as u32, style.unknown);
        let mut stats = RenderStats::default();

        match self.assets() {
            Some(assets) if !assets.terrain.is_empty() => {
                stats.map_tiles = assets.terrain.render_into(&mut map, &viewport);
            }
            _ => {
                stats.map_tiles =
                    render_map_into(&mut map, &self.state_ref().map(), &viewport, &style);
            }
        }

        self.draw_grid_overlay(&mut map, &viewport, &mut stats);
        self.draw_units(&mut map, &viewport, &mut stats, None);
        // 3Р“вЂ”3 buttons of the left panel: overlays over the sprites, like
        // `DrawMap_RenderUnitDisplays`/`GameManager_RenderScanRangeIndicators`.
        self.draw_toggle_overlays(&mut map, &viewport, &mut stats);
        // `GameManager_RenderSurveyIndicator` (`game_manager.cpp:991-995`) РІР‚вЂќ
        // **after** the units, so the resource icons lie over buildings.
        self.draw_resource_icons(&mut map, &viewport, &mut stats);
        // Construction markers РІР‚вЂќ around the selected builder/building
        // (`DrawMap_RenderBuildMarker`); C++ draws them in
        // `DrawMap_RedrawDirtyZones` last, over the resource icons.
        self.draw_build_markers(&mut map, &viewport, &mut stats);
        // Construction site ghost РІР‚вЂќ into the map area, like `GameManager_TempTape`
        // (`ORDER_STATE_SELECT_SITE`), before the cell is confirmed.
        self.draw_build_site_ghost(&mut map, &viewport, &mut stats);
        // Route preview вЂ” over the units, like `GameManager_RenderScanRangeIndicators`
        // (`game_manager.cpp:936-951`): `path->Draw()` of the selected unit. In
        // windowed mode `render_hud` is not called (the panels are drawn by
        // `FRAMEPIC`), so without this the arrows were visible only in the
        // full-screen fallback.
        self.draw_selected_unit_path(&mut map, &viewport, &mut stats);
        crate::chrome::blit_map(&mut frame, &map);

        // Panels over the map: buttons, portrait, statistics, grid, zoom,
        // coordinates РІР‚вЂќ like `GameManager_Draw*` in the original.
        if let Some(panels) = self.assets().and_then(|assets| assets.panels.as_ref()) {
            panels.draw(&mut frame, self);
        }

        // Selected builder hint РІР‚вЂќ the `MESSAGE_BOX_INFO` message window
        // at the start of the map area (`MessageManager_DrawMessage`).
        self.draw_status_message(&mut frame, area);

        // `BuildMenu` РІР‚вЂќ a modal screen over the frame (C++ `AbstractMenu`
        // closes the main menu: `GameManager_DisableMainMenu`).
        self.draw_build_menu(&mut frame);
        // Save/load window (P3).
        self.draw_save_load(&mut frame);
        // Quick save/load `OKCancelMenu` (S1.3).
        self.draw_save_confirm(&mut frame);
        // Upgrades screen (P6).
        self.draw_upgrade_menu(&mut frame);
        // Research screen (P7) and the help window over it.
        self.draw_research_menu(&mut frame);
        // Mine extraction allocation dialog РІР‚вЂќ the full-screen `ALLOCFRM`.
        self.draw_alloc_menu(&mut frame);
        // X-FER window (`XFERPIC`) РІР‚вЂќ over the map and panels.
        self.draw_transfer_menu(&mut frame);
        // Selected unit command menu РІР‚вЂќ over the map and panels.
        self.draw_unit_menu(&mut frame);
        // Repair shop slot list (`RepairShopMenu`) РІР‚вЂќ the full-screen `DEPOTFRM`/`HANGRFRM`.
        self.draw_repair_shop(&mut frame);
        // Unit statistics window (`UnitStats_Menu`) РІР‚вЂќ the full-screen `STATS`.
        self.draw_unit_stats(&mut frame);
        // `Preferences`/`Setup` dialog (`OptionsMenu`, `PREFSPIC`).
        self.draw_preferences(&mut frame);
        self.draw_help(&mut frame);
        // Building self-destruct dialog (UnitsManager_SelfDestructMenu).
        self.draw_self_destruct(&mut frame);
        // Pause dialog (PauseMenu_Menu -> DialogMenu, Alt+P) РІР‚вЂќ over everything.
        self.draw_pause_dialog(&mut frame);

        (frame, stats)
    }

    /// Draws the unit command menu over the frame (C++ `GameManager_InitPopupButtons`).
    ///
    /// The buttons are the same `UNTBTN_U`/`UNTBTN_D` as in C++; the background is not filled:
    /// in the original the popup is drawn right over the map (the `WINDOW_POPUP_BUTTONS`
    /// window only limits the redraw area).
    pub(crate) fn draw_unit_menu(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.unit_menu(), self.assets()) else {
            return;
        };
        let palette = UiPalette::from_assets(assets)
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));

        if let Some(palette) = palette {
            menu.draw(frame, assets, &palette);
        }
    }

    /// Draws the open repair shop slot list (`RepairShopMenu`, `DEPOTFRM`/`HANGRFRM`).
    ///
    /// A full-screen modal like `AllocMenu`: the window palette is the `FRAMEPIC`
    /// one (`Window::palette_from_image = false`), falling back to the menu palette.
    pub(crate) fn draw_repair_shop(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.repair_shop(), self.assets()) else {
            return;
        };
        let palette = UiPalette::from_assets(assets)
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));

        if let Some(palette) = palette {
            menu.draw(
                frame,
                assets.repair_shop.as_deref(),
                &palette,
                self.state_ref(),
            );
        }
    }

    /// Draws the open unit statistics window (`UnitStats_Menu`, `STATS`).
    ///
    /// The window covers the whole 640Р“вЂ”480 frame, so it is drawn after the map
    /// and the panels; the help window (`?`/`Help`) is drawn after it, exactly
    /// like `HelpMenu_Menu("STATS_SETUP")` from `UnitStatsMenu::Run`.
    pub(crate) fn draw_unit_stats(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.unit_stats(), self.assets()) else {
            return;
        };
        let fallback = UiPalette::from_assets(assets)
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));

        menu.draw(
            frame,
            assets.unit_stats.as_ref(),
            assets.cargo.as_ref(),
            fallback,
        );
    }

    /// Draws the open upgrades screen over the frame (P6).
    pub(crate) fn draw_upgrade_menu(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.upgrade_menu(), self.assets()) else {
            return;
        };

        menu.draw(frame, assets, self.state_ref());
    }

    /// Draws the open research screen over the frame (P7).
    pub(crate) fn draw_research_menu(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.research_menu(), self.assets()) else {
            return;
        };

        menu.draw(frame, assets, self.state_ref());
    }

    /// Draws the open mine extraction allocation dialog (C++ `AllocMenu`).
    pub(crate) fn draw_alloc_menu(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.alloc_menu(), self.assets()) else {
            return;
        };

        menu.draw(frame, assets);
    }

    /// Draws the open X-FER window (C++ `TransferMenu`, `XFERPIC`).
    pub(crate) fn draw_transfer_menu(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.transfer_menu(), self.assets()) else {
            return;
        };

        menu.draw(frame, assets, self.state_ref());
    }

    /// Draws the open `Preferences`/`Setup` dialog (C++ `OptionsMenu`).
    pub(crate) fn draw_preferences(&self, frame: &mut RgbaBuffer) {
        let (Some(dialog), Some(assets)) = (self.preferences_dialog(), self.assets()) else {
            return;
        };

        dialog.draw(frame, assets);
    }

    /// Draws the open help window over the frame (P7).
    ///
    /// The palette РІР‚вЂќ the window palette (`FRAMEPIC`), like the other modal windows;
    /// if there is no chrome РІР‚вЂќ the menu palette.
    pub(crate) fn draw_help(&self, frame: &mut RgbaBuffer) {
        let (Some(help), Some(assets)) = (self.help_screen(), self.assets()) else {
            return;
        };
        let fallback = UiPalette::from_assets(assets)
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));

        help.draw(frame, assets.help.as_ref(), fallback);
    }

    /// Draws the open save/load window over the frame (P3).
    pub(crate) fn draw_save_load(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.save_load(), self.assets()) else {
            return;
        };

        menu.draw(frame, assets);
    }

    /// Draws the quick save/load `OKCancelMenu` (C++ `OKCancelMenu`, S1.3) over the
    /// frame.
    pub(crate) fn draw_save_confirm(&self, frame: &mut RgbaBuffer) {
        let Some(confirm) = self.save_confirm() else {
            return;
        };
        let fallback = self
            .assets()
            .and_then(|assets| UiPalette::from_assets(assets))
            .or_else(|| {
                self.assets()
                    .and_then(|assets| assets.menu.as_ref().map(|menu| menu.palette))
            });

        confirm.draw(
            frame,
            self.assets().and_then(|assets| assets.help.as_ref()),
            self.assets().and_then(|assets| assets.transfer.as_ref()),
            fallback,
        );
    }

    /// Draws the building self-destruct dialog (`UnitsManager_SelfDestructMenu`).
    pub(crate) fn draw_self_destruct(&self, frame: &mut RgbaBuffer) {
        let Some(menu) = self.self_destruct() else {
            return;
        };
        let fallback = self
            .assets()
            .and_then(|assets| UiPalette::from_assets(assets))
            .or_else(|| {
                self.assets()
                    .and_then(|assets| assets.menu.as_ref().map(|menu| menu.palette))
            });

        menu.draw(
            frame,
            self.assets().and_then(|a| a.self_destruct.as_ref()),
            fallback,
        );
    }

    /// Draws the pause dialog (`PauseMenu_Menu` РІвЂ вЂ™ `DialogMenu`, `Alt+P`).
    pub(crate) fn draw_pause_dialog(&self, frame: &mut RgbaBuffer) {
        let Some(dialog) = self.pause_dialog() else {
            return;
        };
        let fallback = self
            .assets()
            .and_then(|assets| UiPalette::from_assets(assets))
            .or_else(|| {
                self.assets()
                    .and_then(|assets| assets.menu.as_ref().map(|menu| menu.palette))
            });

        dialog.draw(frame, self.assets().and_then(|a| a.help.as_ref()), fallback);
    }

    /// Draws the open `BuildMenu` over the frame (P2).
    pub(crate) fn draw_build_menu(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.build_menu(), self.assets()) else {
            return;
        };

        menu.draw(frame, assets, self.state_ref(), self.hud().mouse_position);
    }

    /// Units into the buffer through the camera: only those visible and known to the team.
    /// `DrawMap_RenderMapTiles(..., GameManager_DisplayButtonGrid)`
    /// (`game_manager.cpp:1177`): the cell grid over the tiles and **under** the units.
    ///
    /// `complex_grid_overlay` РІР‚вЂќ the `settings` setting (`settings.cpp`), not ported
    /// in the port; the C++ default is taken (`0` РІР‚вЂќ a simple grid
    /// in one color, `DrawMap_RenderSimpleGridOverlay`).
    fn draw_grid_overlay(
        &self,
        frame: &mut RgbaBuffer,
        viewport: &max_render::viewport::Viewport,
        stats: &mut RenderStats,
    ) {
        if !self.hud().panels.grid {
            return;
        }

        let Some(palette) = self.assets().and_then(UiPalette::from_assets) else {
            return;
        };

        stats.overlay_pixels += crate::overlays::draw_grid(frame, self, viewport, &palette, false);
    }

    /// 3Р“вЂ”3 button overlays (`DrawMap_RenderUnitDisplays`,
    /// `GameManager_RenderScanRangeIndicators`): COLORS/STATUS/HITS/AMMO/NAMES and
    /// the RANGE/SCAN circles.
    fn draw_toggle_overlays(
        &self,
        frame: &mut RgbaBuffer,
        viewport: &max_render::viewport::Viewport,
        stats: &mut RenderStats,
    ) {
        let Some(palette) = self.assets().and_then(UiPalette::from_assets) else {
            return;
        };

        stats.overlay_pixels +=
            crate::overlays::draw_unit_overlays(frame, self, viewport, &palette);
        stats.overlay_pixels += crate::overlays::draw_range_scan(frame, self, viewport, &palette);
    }

    fn draw_units(
        &self,
        frame: &mut RgbaBuffer,
        viewport: &max_render::viewport::Viewport,
        stats: &mut RenderStats,
        mut hd: Option<&mut HdPresenter>,
    ) {
        let mut positions = Vec::new();
        let mut visible = Vec::new();
        // A unit sprite may hang into the frame from the edge even if its cell
        // is already outside the visible range: we take a margin of 2 cells.
        let range = viewport.visible_tiles();
        let margin = 2;

        for id in self.state_ref().unit_ids_in_order() {
            let Some(unit) = self.state_ref().unit(id) else {
                stats.units_skipped += 1;
                continue;
            };
            // Visibility and drawing order РІР‚вЂќ by the visual position: while
            // the sprite walks the route, the simulation cell may already be
            // ahead (`update_visual_units`).
            let (visual_x, visual_y) = self
                .visual_position(id)
                .unwrap_or((f32::from(unit.grid.x), f32::from(unit.grid.y)));
            let tile_x = visual_x.round() as i16;
            let tile_y = visual_y.round() as i16;
            let in_range = range.is_some_and(|range| {
                tile_x >= range.x0 - margin
                    && tile_y >= range.y0 - margin
                    && tile_x <= range.x1 + margin
                    && tile_y <= range.y1 + margin
            });
            if !in_range {
                continue;
            }
            // Visibility (fog of war); the debug `--no-fow` mode draws every unit
            // (`GameManager_AllVisible` observation mode). While the frame follows
            // the visual anchors, a foreign unit must also be inside the scan of an
            // own unit **at its visual cell** РІР‚вЂќ the radar of a moving unit reveals
            // the enemy when the sprite arrives (`App::presented_visible`).
            if !self.no_fow() && !self.presented_visible(unit) {
                continue;
            }
            // `UnitInfoGroup::IsRelevant` (`unitinfogroup.cpp:59`): idle units
            // are not drawn (except the QuickBuild preview, which we do not have), and at
            // a small scale (`Gfx_ZoomLevel < 8`) `GROUND_COVER` is not drawn either.
            if unit.orders == max_core::enums::UnitOrder::Idle
                || (unit.flags & unit_flags::GROUND_COVER != 0 && viewport.zoom() < 8)
            {
                stats.units_skipped += 1;
                continue;
            }
            positions.push((tile_x, tile_y));
            visible.push(id);
        }

        // Selection indicator palette: `0xFF`/`0x00` indices of the game window
        // (`GameManager_MarkerColor`). Without assets РІР‚вЂќ white/black.
        let selector_palette = match self.assets() {
            Some(assets) => window_palette(assets),
            None => fallback_selector_palette(),
        };

        for index in draw_order(&positions) {
            let Some(unit) = self.state_ref().unit(visible[index]) else {
                continue;
            };
            // Visual position РІР‚вЂќ a fractional cell walking the route
            // (see `App::update_visual_units`). The anchor РІР‚вЂќ the cell center in world
            // pixels (for `BUILDING` buildings РІР‚вЂќ +31, like `UnitInfo::SetPosition`),
            // then the camera converts it to screen coordinates.
            let (grid_x, grid_y) = self
                .visual_position(visible[index])
                .unwrap_or((f32::from(unit.grid.x), f32::from(unit.grid.y)));
            // Rocket/torpedo trail particles (`Paths_DrawMissile`) and the
            // projectiles themselves (`AirPath::Execute` РІР‚вЂќ pixel step): C++ spreads
            // only the draw zones, the port keeps this in pixel `x/y`.
            let (world_x, world_y) = effect_pixel_anchor(unit)
                .or_else(|| missile_pixel_anchor(unit))
                .or_else(|| explosion_pixel_anchor(unit))
                .or_else(|| air_pixel_anchor(unit, (grid_x, grid_y)))
                .unwrap_or_else(|| unit_anchor_world(unit, (grid_x, grid_y)));
            // Flight bob (`UnitInfo::ShakeAir`, `unitinfo.cpp:4636-4678`): a
            // hovering aircraft sways by up to two pixels along a rotating
            // direction, the shadow by three times as much (see `hover_offset`).
            // `dt = 0` (QA/headless) РІР‚вЂќ zero, the goldens do not drift.
            let (bob_x, bob_y) = self.hover_offset(visible[index]);
            // `UnitInfo::PositionInTape` + `OffsetDrawZones` (`unitinfo.cpp:4478`):
            // the constructor shifts by 8 px per tick toward the 2Р“вЂ”2 tape center, 32 px in total
            // (half a cell) РІР‚вЂќ to the center of the construction site. This is a frame
            // shift of the sprite, the builder grid does not change.
            let (tape_offset_x, tape_offset_y) =
                crate::buildsite::tape_center_offset(self.state_ref(), unit);
            let anchor = viewport.world_to_screen(
                world_x + tape_offset_x + bob_x,
                world_y + tape_offset_y + bob_y,
            );

            let image_base = image_base_for(
                unit,
                self.state_ref()
                    .teams()
                    .get(unit.team.0 as usize)
                    .map(|team| team.team_clan),
            );
            // Body angle РІР‚вЂќ of the current visual segment (`visual_angle`):
            // the simulation turns the unit at the route cell, while the sprite walks
            // it at the real speed; otherwise the frame would "run ahead" and the unit
            // would face sideways while moving straight.
            let body_angle = self.visual_angle(visible[index]).unwrap_or(unit.angle);
            // Frame-by-frame animation of the builder and `ANIMATED` buildings РІР‚вЂќ the
            // visual clock of `App` (real `dt`), not part of the snapshot/CRC. Mines and
            // factories are **not** advanced by time (oracle `unitinfo.cpp:2768` holds
            // `image_base + 1`; DOS capture Р’В§7.2): their blinking comes from the palette
            // cycles. `dt = 0` (QA/headless) gives the static C++ frame (`+16` for the
            // builder, the base/powered-on building frame) РІР‚вЂќ so the goldens do not drift.
            let animation = self.unit_animation(visible[index]);
            // Complex connectors (`UnitInfo::RenderWithConnectors`): the stored
            // mask from the save РІв‚¬Р„ computed by neighbors (see [`drawn_connector_mask`]).
            let connectors = drawn_connector_mask(self.state_ref(), unit);
            // C++ `settings["effects"]` (`settings.cpp:128`): `effects = false`
            // disables frame-by-frame sprite animations (`UnitInfo::Animate`,
            // `units_manager.cpp:2857/3030`) РІР‚вЂќ the frame is static.
            let effects = self.preferences.effects;
            let zoom = self.unit_render_zoom(visible[index], viewport.zoom());
            // Presentation fields (shadow/brightness) РІР‚вЂќ a separate table
            // `GameState::presentation` (phase 5.3/B), not `UnitInfo`.
            let mut presentation = self
                .state_ref()
                .presentation(visible[index])
                .cloned()
                .unwrap_or_default();
            // `UnitInfo::ShakeAir` shifts `shadow_offset -= 2 * offset` on top of the
            // hull's `OffsetDrawZones(offset)`. The anchor above already carries
            // `offset`, so the stored value takes `-2 * offset` and the shadow ends
            // up three times as far from its base as the hull.
            if bob_x != 0 || bob_y != 0 {
                presentation.shadow_offset = bobbed_shadow_offset(&presentation, (bob_x, bob_y));
            }
            let drawn = self.assets().and_then(|assets| {
                // Frame palette: a snapshot with palette cycles or the original
                // system palette (`dt = 0` РІР‚вЂќ no changes).
                let palette = self.animated_palette().unwrap_or(&assets.system_palette);

                // HD mode: the R4 sprite bank of the type, with the original
                // sprite renderer as the per-asset fallback of every frame.
                if let Some(presenter) = hd.as_deref_mut() {
                    if presenter.has_sprite(unit.unit_type) {
                        return draw_unit_sprites_with_animation_hd(
                            assets,
                            palette,
                            unit,
                            &presentation,
                            image_base,
                            body_angle,
                            animation,
                            effects,
                            connectors,
                            frame,
                            anchor,
                            zoom,
                            presenter,
                            self.color_cycles(),
                        );
                    }
                }

                draw_unit_sprites_with_animation(
                    assets,
                    palette,
                    unit,
                    &presentation,
                    image_base,
                    body_angle,
                    animation,
                    effects,
                    connectors,
                    frame,
                    anchor,
                    zoom,
                )
            });

            match drawn {
                Some(_) => stats.units_drawn += 1,
                None => {
                    draw_marker(frame, anchor, unit.team.0, viewport.zoom());
                    stats.units_marker += 1;
                }
            }

            // `GameManager_DrawUnitSelector` (`game_manager.cpp:744`) from
            // `DrawMap_RenderUnit` (`drawmap.cpp:813-833`): for the selected unit РІР‚вЂќ
            // three nested corner frames with the blinking `GameManager_MarkerColor` color.
            if self.hud().selected == Some(visible[index]) {
                if draw_unit_selector(
                    frame,
                    &selector_palette,
                    anchor,
                    unit.is_quad(),
                    viewport.zoom(),
                    self.marker_color_index(),
                ) > 0
                {
                    stats.selector_units += 1;
                }
            }
        }
    }

    /// Construction site ghost (`GameManager_TempTape`) into the map buffer.
    ///
    /// C++ spawns a real `LRGTAPE`/`SMLTAPE` tape for the time of
    /// `ORDER_STATE_SELECT_SITE` and moves it after the cursor; the port draws **the same
    /// sprite** without writing into the match state РІР‚вЂќ the order is not issued before LMB, and
    /// the unit counter/CRC/save do not change.
    fn draw_build_site_ghost(
        &self,
        frame: &mut RgbaBuffer,
        viewport: &max_render::viewport::Viewport,
        stats: &mut RenderStats,
    ) {
        let (Some(site), Some(assets)) = (self.build_site(), self.assets()) else {
            return;
        };

        if site.draw_ghost(frame, assets, viewport).is_some() {
            stats.units_drawn += 1;
        }
    }

    /// Route arrows of the selected unit вЂ” `GameManager_RenderScanRangeIndicators`
    /// (`game_manager.cpp:936-951`): `unit->path->Draw(unit, window)` for the own,
    /// visible, moving selected unit.
    ///
    /// Green markers are reachable this turn, blue ones in the following turns
    /// ([`draw_path_markers`]). While the move order has been clicked but the next
    /// tick has not applied it yet (no simulation path), the HUD preview line of
    /// [`crate::hud::draw_path_preview`] is shown as a fallback.
    fn draw_selected_unit_path(
        &self,
        frame: &mut RgbaBuffer,
        viewport: &max_render::viewport::Viewport,
        stats: &mut RenderStats,
    ) {
        let Some(assets) = self.assets() else {
            return;
        };

        let state = self.state_ref();
        let team = state.player_team();

        let mut markers = Vec::new();

        if let Some(unit) = self.hud().selected.and_then(|id| state.unit(id)) {
            if draws_unit_path(unit, team) {
                markers = state.path_markers(unit);
            }
        }

        if !markers.is_empty() {
            let palette = window_palette(assets);
            stats.route_markers = draw_path_markers(frame, viewport, &markers, &palette);
            return;
        }

        // Fallback: the clicked order that the next tick has not applied yet.
        if let Some(preview) = &self.hud.preview {
            crate::hud::draw_path_preview(frame, viewport, preview);
        }
    }

    /// `Access_IsSurveyorOverlayActive` (`access.cpp:341-351`): the selected
    /// unit РІР‚вЂќ own team, type `SURVEYOR` and order **not** `ORDER_IDLE`.
    ///
    /// In C++ this is cached in `GameManager_IsSurveyorSelected` on unit selection
    /// (`GameManager_MenuUnitSelect`, `game_manager.cpp:7165`); the port computes
    /// the value from the current state РІР‚вЂќ the same thing, but without a "stuck" flag
    /// if the scout managed to become `ORDER_IDLE` while already selected.
    pub fn selected_surveyor_active(&self) -> bool {
        self.hud()
            .selected
            .and_then(|id| self.state_ref().unit(id))
            .is_some_and(|unit| {
                unit.team == self.state_ref().player_team()
                    && unit.unit_type == UnitType::Surveyor
                    && unit.orders != UnitOrder::Idle
            })
    }

    /// `GameManager_RenderSurveyIndicator` (`game_manager.cpp:991`):
    /// `GameManager_DisplayButtonSurvey || GameManager_IsSurveyorSelected`.
    ///
    /// The SURVEY button (`hud.panels.survey` РІР‚вЂќ `GameManager_DisplayButtonSurvey`,
    /// `game_manager.cpp:3985`) and the selected scout enable showing resource
    /// icons; without them the icons are not drawn at all.
    pub fn survey_overlay_active(&self) -> bool {
        self.hud().panels.survey || self.selected_surveyor_active()
    }

    /// Current selection indicator color РІР‚вЂќ `GameManager_MarkerColor`
    /// (`game_manager.cpp:222/1191`): `0xFF` (white) or `0x00` (black),
    /// toggled every [`UNIT_SELECTOR_BLINK_SECONDS`] (210 ms).
    pub fn marker_color_index(&self) -> u8 {
        self.marker_color_index
    }

    /// Survey markers РІР‚вЂќ `DrawMap_RenderSurveyDisplay` (`drawmap.cpp:1065`) +
    /// `Survey_RenderMarkers` (`survey.cpp:123`).
    ///
    /// C++ (`game_manager.cpp:991-995`): `GameManager_RenderSurveyIndicator`
    /// calls the drawing only if `GameManager_DisplayButtonSurvey`
    /// (the panel **SURVEY** button) or `GameManager_IsSurveyorSelected`
    /// (`Access_IsSurveyorOverlayActive`, `access.cpp:341`: a selected
    /// `SURVEYOR` of own team not in `ORDER_IDLE`). Inside
    /// `Survey_RenderMarkers` the mask is the team `hash_team_id` (the `MAXSURVEY` cheat
    /// gives `0xFFFF` and is not enabled in the port), so a marker appears only on
    /// a **surveyed** cell (`Survey_SurveyArea` sets the bit when the scout
    /// passes by): the `*MK1..16` amount of a known deposit, or the `*MK0` circle
    /// of an explored cell **without** a resource (`Survey_RenderMarker`,
    /// `survey.cpp:52-73`).
    fn draw_resource_icons(
        &self,
        frame: &mut RgbaBuffer,
        viewport: &max_render::viewport::Viewport,
        stats: &mut RenderStats,
    ) {
        if !self.survey_overlay_active() {
            return;
        }

        let Some(assets) = self.assets() else {
            return;
        };

        if assets.terrain.is_empty() {
            return;
        }

        let style = TileStyle::default();
        let (pixels, cells) = render_resources(
            frame,
            assets,
            &self.state_ref().map(),
            viewport,
            &style,
            self.state_ref().player_team().0,
            // `GameManager_MaxSurvey` РІР‚вЂќ a cheat (`CHEAT_CODE_MAXSURVEY`), not implemented
            // in the port: we show only surveyed cells.
            false,
        );

        stats.resource_icons += pixels;
        stats.resource_icon_cells += cells;
    }

    /// Blinking construction markers `BLDMRK1..5` РІР‚вЂќ `DrawMap_RenderBuildMarker`
    /// (`drawmap.cpp:353`) + `DrawMap_RedrawUnit` (`drawmap.cpp:304`).
    ///
    /// Drawn when an own team unit is selected in `ORDER_IDLE` (a completed
    /// building waits for a manager) or a mobile builder in
    /// `ORDER_BUILD`/`ORDER_STATE_UNIT_READY` (completed and waiting for the order to leave
    /// the site): the symbols are placed on the cells around the **parent** of the selected
    /// unit (`Access_IsAccessible` filters occupied ones), and a click on such a cell РІР‚вЂќ
    /// a regular move order, which in this state gives
    /// `ORDER_ACTIVATE` (`game_manager.cpp:6879`). The hint `_(276c)` directly
    /// refers to these symbols.
    fn draw_build_markers(
        &self,
        frame: &mut RgbaBuffer,
        viewport: &max_render::viewport::Viewport,
        stats: &mut RenderStats,
    ) {
        let Some(selected) = self.hud().selected else {
            return;
        };
        let Some(unit) = self.state_ref().unit(selected) else {
            return;
        };
        let Some(assets) = self.assets() else {
            return;
        };

        let cells = build_marker_cells(self.state_ref(), unit);

        if cells.is_empty() {
            return;
        }

        let Some(image) = assets
            .build_markers
            .get(usize::from(self.build_marker_step))
        else {
            return;
        };
        let palette = window_palette(assets);
        let zoom = viewport.zoom() as i32;

        for cell in cells {
            let rect = viewport.tile_rect(cell.x, cell.y);

            if rect.width <= 0 || rect.height <= 0 {
                continue;
            }

            let centre = (rect.ulx + rect.width / 2, rect.uly + rect.height / 2);

            // `WindowManager_RescaleSimpleImage(image, Gfx_MapScalingFactor)`:
            // 62Р“вЂ”62 РІР‚вЂќ the full 64 px cell frame, scale `zoom/64`.
            if max_render::map_render::blit_simple_image(frame, image, &palette, centre, zoom, 64)
                > 0
            {
                stats.build_markers += 1;
            }
        }
    }
}

/// Cells where `DrawMap_RedrawUnit` (`drawmap.cpp:304`) draws construction
/// markers: around the **parent** of the selected unit.
///
/// C++ condition: `unit->team == PlayerTeam` and `(order == ORDER_IDLE ||
/// (order == ORDER_BUILD && state == ORDER_STATE_UNIT_READY && !(flags &
/// STATIONARY)))`, the parent exists and it is not `AIRTRANS`. The start РІР‚вЂќ
/// `(parent.x - 1, parent.y + 1)`, for a building `(+1 in y, limit = 3)`, then
/// four directions (`N, E, S, W` РІР‚вЂќ `direction += 2`) for `limit` steps,
/// cumulatively; only passable cells get into the list
/// (`Access_IsAccessible(... AccessModifier_SameClassBlocks)`).
pub fn build_marker_cells(
    state: StateRef<'_>,
    unit: &max_core::unit::UnitInfo,
) -> Vec<max_core::geo::Point> {
    if unit.team != state.player_team() {
        return Vec::new();
    }

    let eligible = unit.orders == UnitOrder::Idle
        || (unit.orders == UnitOrder::Build
            && unit.state == UnitOrderState::UnitReady
            && unit.is_mobile());

    if !eligible {
        return Vec::new();
    }

    let Some(parent) = unit.parent_unit.and_then(|id| state.unit(id)) else {
        return Vec::new();
    };

    if parent.unit_type == UnitType::AirTransport {
        return Vec::new();
    }

    let building = parent.is_quad();
    let mut position = max_core::geo::Point::new(parent.grid.x - 1, parent.grid.y + 1);
    let limit = if building {
        position.y += 1;
        3
    } else {
        2
    };
    let mut cells = Vec::new();

    for direction in (0..8).step_by(2) {
        for _ in 0..limit {
            position += max_core::geo::DIRECTION_OFFSETS[direction];

            if state.is_accessible(
                unit.unit_type,
                state.player_team(),
                position,
                max_core::access::ACCESS_MODIFIER_SAME_CLASS_BLOCKS,
            ) > 0
            {
                cells.push(position);
            }
        }
    }

    cells
}

/// Window system palette for simple images (resource markers, cursors).
///
/// In game this is the `FRAMEPIC` palette (C++ sets it as the system one when loading the chrome);
/// without the chrome РІР‚вЂќ `COLOR.PAL` as a reasonable fallback.
pub fn window_palette(assets: &crate::assets::Assets) -> PaletteRgba {
    if let Some(chrome) = assets.chrome.as_ref() {
        return UiPalette::from_big_image(&chrome.frame).palette_rgba();
    }

    let mut palette = [[0u8, 0, 0, 255]; 256];

    for (index, entry) in palette.iter_mut().enumerate() {
        let [r, g, b] = assets.palette.rgb(index as u8);

        *entry = [r, g, b, 255];
    }

    palette
}

/// Resource markers: real `RAWMSK*`/`FUELMK*`/`GOLDMK*`, otherwise dots.
///
/// Returns `(pixels, cells)`.
#[allow(clippy::too_many_arguments)]
fn render_resources(
    frame: &mut RgbaBuffer,
    assets: &crate::assets::Assets,
    map: &max_core::map::Map,
    viewport: &max_render::viewport::Viewport,
    style: &TileStyle,
    team: u8,
    max_survey: bool,
) -> (u32, u32) {
    if assets.resource_markers.is_ready() {
        max_render::map_render::render_resource_icons_into(
            frame,
            map,
            viewport,
            &assets.resource_markers,
            &window_palette(assets),
            team,
            max_survey,
        )
    } else {
        // Simplified rendering (without `MAX.RES` data): resource dots are drawn
        // always РІР‚вЂќ this is a debug map, not a game frame.
        (
            max_render::map_render::render_resources_into(frame, map, viewport, style),
            0,
        )
    }
}

/// 6Р“вЂ”6 unit marker at the `anchor` point (cell center), scaled with
/// the map (`zoom`, 64 РІР‚вЂќ 1:1).
fn draw_marker(frame: &mut RgbaBuffer, anchor: (i32, i32), team: u8, zoom: u32) {
    let color = MARKER_COLORS
        .get(usize::from(team))
        .copied()
        .unwrap_or(MARKER_COLORS[4]);
    let radius = ((6 * i64::from(zoom.max(1))) / 64).max(2) as i32;

    for y in -radius..=radius {
        for x in -radius..=radius {
            frame.set(anchor.0 + x, anchor.1 + y, color);
        }
    }
}

/// Writes a [`RgbaBuffer`] into a binary PPM (`P6`) РІР‚вЂќ viewing a frame without SDL.
///
/// PPM was chosen because the format is three header lines and raw RGB, without
/// codecs and dependencies: a frame from a headless run can be opened by any
/// image viewer.
pub fn write_ppm(path: impl AsRef<Path>, buffer: &RgbaBuffer) -> io::Result<()> {
    let path = path.as_ref();
    let file = fs::File::create(path)?;
    let mut writer = io::BufWriter::new(file);

    write!(writer, "P6\n{} {}\n255\n", buffer.width(), buffer.height())?;

    let mut rgb = Vec::with_capacity(buffer.len() * 3);
    for pixel in buffer.pixels().chunks_exact(4) {
        rgb.extend_from_slice(&pixel[..3]);
    }

    writer.write_all(&rgb)?;
    writer.flush()
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
