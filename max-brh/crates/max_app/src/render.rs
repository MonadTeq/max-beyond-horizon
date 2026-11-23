//! Frame without SDL: map + units in [`RgbaBuffer`].
//!
//! Only the **pure** parts of [`max_render`] are collected here:
//! [`render_map_into`] (tiles/surfaces/resources), [`SpriteRenderer`]
//! (palette, team LUTs, shadows) and [`draw_order`] (unit order by `y`,
//! §5.4). The SDL layer (`max_render::sdl_window`) is connected exclusively in
//! `src/main.rs` behind the `sdl` feature, so the application and its tests draw frames
//! without a window and without a display.
//!
//! If the assets are not loaded ([`App::init_assets`] was not called) or the type
//! resource is absent from [`crate::assets::UNIT_SPRITES`], the unit is drawn as a square
//! team-colored marker. In the game data there is exactly one such type — `MASTER`
//! (`UNIT_TYPES_WITHOUT_SPRITE_DATA`); the map is always drawn with
//! the real surface/resource colors from [`TileStyle`].
//!
//! The common unit drawing point — [`draw_unit_sprites`]: it is used by the map
//! (`App::draw_units`) and the HUD portrait (`panels::draw_portrait`), so all
//! 93 types are drawn the same way (body, shadow, turret with an offset).

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::map_render::{render_map_into, TileStyle};
use max_render::palette::{ColorLut, TeamColors, FULL_BRIGHTNESS_LEVEL};
use max_render::sprite_atlas::SpriteRenderer;
use max_render::terrain::PaletteRgba;
use max_render::viewport::draw_order;

use max_core::enums::{unit_flags, TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::unit::UnitPresentation;
use max_core::universe::{Dispatcher, StateRef};

use crate::panels::UiPalette;
use crate::App;

/// Shift of the building (`flags & BUILDING`) anchor from the cell center.
///
/// C++ `UnitInfo::SetPosition` (`unitinfo.cpp:1301`): `x = grid*64 + 32`, then
/// for `flags & BUILDING` — `x += (GFX_MAP_TILE_SIZE / 2) - 1`, i.e. **+31**.
/// A 2×2 building with hotspot `(58,55)` (`MININGST`) is drawn in its 2×2
/// rectangle, not half a cell up-left. Without this the mine "drifted"
/// onto the generator cell and their sprites overlapped.
pub const BUILDING_ANCHOR_OFFSET: i32 = (64 / 2) - 1;

/// Shift of the builder frame into the working pose (`DrawSpriteFrame(image_index + 16)`).
///
/// C++ `UnitInfo::StartBuilding` (`unitinfo.cpp:2789`) for the engineer and
/// `UnitInfo::PositionInTape` (`unitinfo.cpp:4451`) for the constructor. In
/// the `ENGINEER` sprites there are 32 frames, `CONSTRCT` — 40, so `angle + 16` always
/// falls into the working animation block.
pub const BUILDER_WORK_FRAME_OFFSET: usize = 16;

/// Step of the constructor working cycle (`DrawSpriteFrame(image_index + 8)`).
///
/// C++ `UnitsManager_ProcessOrderBuild` (`units_manager.cpp:2856`): while the order is
/// `ORDER_BUILD`/`ORDER_STATE_BUILD_IN_PROGRESS`, the constructor's frame shifts
/// by `+8` every second tick; if `image_index + 8 >= 40`, instead of
/// that `image_index - 16` is taken — i.e. the frames loop
/// `+16 → +24 → +32 → +16`. For the engineer the cycle does not advance: it keeps `+16`.
pub const BUILDER_WORK_FRAME_STEP: usize = 8;

/// How many different frames are in the constructor working cycle (`+16`, `+24`, `+32`).
pub const BUILDER_WORK_FRAME_STEPS: u8 = 3;

/// Whether the mine drill frame advance is enabled (frames `1..15`).
///
/// **Deliberately disabled:** the DOS capture (`max-original-dasm/formats/
/// unit-animation.md` §7.2) showed that when idle the mine body is always frame 0
/// (powered on — `image_base + 1`), not a single transition `0 → 1 → … → 15` over
/// ~118 s of game time. Frames 1..15 (drill positions) probably play
/// only during active extraction (§7.4) — this requires a separate
/// capture of active extraction/turn processing. They can only be enabled after
/// such a capture: the constant is the only handle of the rule.
pub const MINING_DRILL_FRAMES_ENABLED: bool = false;

/// Unit frame clock (drawing only, not part of the snapshot/CRC).
///
/// Two independent counters repeat two different C++ mechanisms:
///
/// * `builder_step` — `UnitsManager_ProcessOrderBuild` (`+8`, `-16` when
///   `image_index + 8 >= 40`);
/// * `sentry_step` — `UnitsManager_ProcessOrderSentry` for units with the
///   `ANIMATED` flag: every tick `image_index + 1`, after the last —
///   `image_index = image_base`.
///
/// There are no **factory** frames here: C++ does not advance them by time. A factory with
/// `ORDER_BUILD` keeps frame `image_base + 1` (`UnitInfo::StartBuilding`,
/// `unitinfo.cpp:2789`), after `SpawnNewUnit`/`CancelBuilding` — `image_base`;
/// the visible "blinking" of a powered-on factory comes from **palette cycles**
/// (`World::InitColorCycles`, `world.cpp:542`: slots 9–30, 6/9/2 fps), not from
/// sprite frames (DOS capture §7.2/§7.3 and the `MAX.RES` frames:
/// `TRAINHAL`/`LIGHTPLT`/`AIRPLT` have exactly 2 body frames).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameAnimation {
    /// Number of the builder working cycle frame (`0..BUILDER_WORK_FRAME_STEPS`).
    pub builder_step: u8,
    /// `ProcessOrderSentry` tick number (ANIMATED).
    pub sentry_step: u32,
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
/// factories do not have it — C++ does not animate them by time.
pub fn sentry_animation_active(unit: &max_core::unit::UnitInfo) -> bool {
    unit.flags & unit_flags::ANIMATED != 0 && unit.orders == UnitOrder::Sentry
}

/// Whether a **stationary factory** is working — C++ keeps its frame
/// `image_base + 1` (`UnitInfo::StartBuilding`, `unitinfo.cpp:2789`).
///
/// C++ does not advance factory frames by time: `TRAINHAL`/`LIGHTPLT`/`LANDPLT`/
/// `AIRPLT`/`SHIPYARD` in `MAX.RES` have exactly two body frames (`image_base` —
/// off, `image_base + 1` — working) and 8 connector frames. The change of
/// `image_base` ↔ `image_base + 1` follows the order: `StartBuilding` draws
/// `+1`, `SpawnNewUnit`/`CancelBuilding`/`PowerDown` — the base frame. The visible
/// "blinking" comes from palette cycles (`World::InitColorCycles`), not from frames
/// (`max-original-dasm/formats/unit-animation.md` §7).
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

/// Connector frames: the `CONNECTOR_*` bit → offset from `connector_image_base`.
///
/// The literal order of `UnitInfo::RenderWithConnectors` (`unitinfo.cpp:3151`):
/// first north/south (two connectors each for a 2×2 building), then east/west.
/// The offsets are the same as in C++: `CONNECTOR_NORTH_LEFT → +0`,
/// `NORTH_RIGHT → +4`, `SOUTH_LEFT → +2`, `SOUTH_RIGHT → +6`,
/// `EAST_TOP → +1`, `EAST_BOTTOM → +5`, `WEST_TOP → +3`, `WEST_BOTTOM → +7`.
///
/// It is these frames that the DOS capture took for the mine's "8 drilling particles"
/// (`unit-animation.md` §7.1): `D_MINING.connector_image_base = 16`,
/// `connector_image_count = 8`, and in a dense base all eight bits were set
/// at the mine. For `POWGEN` (`D_SMLBLD`: base 2, 4 connectors) — four
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

/// Team building in a cell — `Access_GetTeamBuilding` (`access.cpp:2157`).
///
/// C++ looks for the first unit of the cell with the same team, not `ORDER_IDLE`, and
/// `IsComplexMember()`; `Hash_MapHash` registers all four cells for 2×2 buildings,
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

/// Building connector mask — **for the frame** (duplicates the simulation
/// `complex::update_connectors`).
///
/// C++ computes the bits when the complex is built/rebuilt and stores them in
/// `UnitInfo::connectors` (in the port this is a simulation field
/// [`max_core::complex::update_connectors`], saved in V70 and included in the
/// snapshot/CRC). This function repeats the same formula over the map **only for
/// the frame**: it does not write to the state. The real mask is merged with it in
/// [`drawn_connector_mask`] — this way the loaded V70 saves, where the bits are already set,
/// and a fresh simulation are drawn the same.
///
/// Rules (exactly as in C++): the neighbor above `(gx, gy-1)` gives `NORTH_LEFT`,
/// above-right `(gx+1, gy-1)` — `NORTH_RIGHT`; below `(gx, gy+size)` —
/// `SOUTH_LEFT`, below-right `(gx+1, gy+size)` — `SOUTH_RIGHT`; right
/// `(gx+size, gy)` — `EAST_TOP`, right-below `(gx+size, gy+1)` —
/// `EAST_BOTTOM`; left `(gx-1, gy)` — `WEST_TOP`, left-below
/// `(gx-1, gy+1)` — `WEST_BOTTOM`. 1×1 buildings (`POWGEN`) have only
/// `NORTH_LEFT`/`SOUTH_LEFT`/`EAST_TOP`/`WEST_TOP`.
pub fn connector_mask_for(
    state: StateRef<'_>,
    unit: &max_core::unit::UnitInfo,
) -> u16 {
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

/// The mask the connectors are actually drawn with: stored ∪ computed.
///
/// The stored mask — from the V70 save/simulation (`UnitInfo::connectors`,
/// [`max_core::complex::update_connectors`]); the computed one — [`connector_mask_for`]
/// (in case the state was loaded without recomputing connectors).
pub fn drawn_connector_mask(
    state: StateRef<'_>,
    unit: &max_core::unit::UnitInfo,
) -> u16 {
    unit.connectors | connector_mask_for(state, unit)
}

/// "Growth" scale from `scaler_adjust`: `2^-scaler` (`UnitInfo::RenderSprite`).
///
/// C++ `UnitInfo::RenderSprite`/`RenderShadow` (`unitinfo.cpp:3085/3050`)
/// divide the frame size by `scaling_factor = 1 << (scaler_adjust + 1)`, i.e.
/// relative to a normal frame (`scaler_adjust = 0`, divisor 2) — `1 / 2^n`:
/// `4 → 1/16`, `3 → 1/8`, `2 → 1/4`, `1 → 1/2`, `0 → 1`. `0` — full size.
pub fn scaler_scale(scaler_adjust: u8) -> f32 {
    let scaler = scaler_adjust.min(4);
    1.0 / f32::from(1u16 << scaler)
}

/// Body frame base taking the clan into account.
///
/// C++ `UnitInfo::UnitInfo` (`unitinfo.cpp:281`): for a mine
/// `image_base = (team_clan - 1) * sizeof(uint16_t)` — 8 clan variants of
/// 2 frames (off/on). The other types take the base from `D_*` (`unit.image_base`).
/// `TeamClan::Random` (0) is not resolved to a specific clan in max_core, so
/// for it and for saves with a zero base clan 1 (base 0) is used — as it was
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

/// Body frame index: `image_base + angle`, and for a powered-on building — +1.
///
/// C++ `UnitsManager_ProcessOrderPowerOn` (`units_manager.cpp:2945`) with
/// `ORDER_POWER_ON` draws `DrawSpriteFrame(image_base + 1)`, `PowerDown` —
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
/// The unit sprite — `image_base + angle` (8 directions), so the frame must
/// be chosen by the angle with which the sprite actually moves on the map
/// (`App::visual_angle`), not only by the simulation one: the visual route
/// lags behind the simulation, and otherwise the frame would turn before the turn cell.
///
/// During construction C++ draws the builder working pose:
/// `DrawSpriteFrame(image_index + 16)` (`unitinfo.cpp:2789` for the engineer and
/// `4451` for the constructor), and `UnitsManager_ProcessOrderBuild` for the constructor
/// additionally advances `+8` (`units_manager.cpp:2856-2868`). The sprites
/// `ENGINEER` (32 frames) and `CONSTRCT` (40) contain these frames, so here
/// exactly `+16` is added while the order is `ORDER_BUILD`, the state —
/// `ORDER_STATE_BUILD_IN_PROGRESS`. After completion (`ORDER_AWAIT`) the frame is
/// base again: C++ returns `DrawSpriteFrame(image_index - 16)`.
pub fn unit_frame_index_for_angle(
    unit: &max_core::unit::UnitInfo,
    image_base: i16,
    image_count: usize,
    angle: u8,
) -> usize {
    let last = image_count.saturating_sub(1);
    let base = usize::try_from(image_base.max(0)).unwrap_or(0);
    let mut index = base.saturating_add(usize::from(angle % 8));

    // C++ `UnitsManager_ProcessOrderPowerOn` (`units_manager.cpp:2945`) and
    // `UnitInfo::StartBuilding` (`unitinfo.cpp:2789`) draw `image_base + 1`
    // (a powered-on building/working factory), `PowerDown`/`SpawnNewUnit`/
    // `CancelBuilding` — `image_base`.
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
/// `firing_recoil_frames` (3; ×5 for `ANTIAIR`, ×2 for `SP_FLAK`/`FASTBOAT`;
/// 8 for `COMMANDO`/`INFANTRY`) and draws the firing frame; each
/// `UnitInfo::ProgressFire` (`:4824`) decrements the counter and at zero returns
/// the normal frame. The counter is a state field, so the flash is deterministic and
/// part of the snapshot/CRC.
pub fn firing_frame_active(unit: &max_core::unit::UnitInfo) -> bool {
    unit.firing_recoil_frames > 0 && unit.flags & unit_flags::HAS_FIRING_SPRITE != 0
}

/// The **body** frame during recoil (`UnitInfo::PrepareFire`).
///
/// * `COMMANDO`/`INFANTRY` — the special 8-frame C++ queue: `DrawSpriteFrame(
///   unit_angle + 104)`, then every `ProgressFire` — `image_index + 8`
///   (unitinfo.cpp:4732-4742/4831-4832), i.e. the frame
///   `104 + angle + 8·(8 − firing_recoil_frames)`;
/// * other units without a turret (`flags & TURRET_SPRITE == 0`) —
///   `DrawSpriteFrame(unit_angle + firing_image_base)`;
/// * units with a turret keep the body unchanged (C++ draws only the turret).
///
/// `None` — the body is drawn with the normal frame ([`unit_frame_index_with_animation`]).
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
        // `unit_angle + 104`, then +8 for each recoil tick (8 → 0).
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
/// toggles `turret_image_index ± 8` (`unitinfo.cpp:4828-4830`), so the
/// flash alternates with the normal turret frame. The sign of the first toggle
/// depends on the parity of the initial counter (15 for `ANTIAIR`, 6 for
/// `SP_FLAK`/`FASTBOAT`) — here it is computed from `firing_recoil_frames`.
///
/// `None` — the unit has no turret/firing frames, the turret is drawn with the normal frame.
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
        // Initial `PrepareFire` counter: 3 × 5 for ANTIAIR, 3 × 2 for the others.
        let initial = if unit.unit_type == UnitType::AntiAir { 15 } else { 6 };

        if initial % 2 == 1 {
            // ANTIAIR: 15 → 14 (even) toggles to firing+8.
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
/// the vacated cell and spreads the **draw zones** over 0/⅓/⅔ of the segment
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

/// Unit frame taking the builder frame-by-frame animation into account (`build_step`).
///
/// `build_step` — number of the constructor working cycle frame (0, 1, 2) from
/// [`crate::App::build_animation_step`]; `0` — the construction start frame `+16`, as
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
/// `16 → 24 → 32 → 16` (for angle 0). `ENGINEER` has no cycle — frame `+16`
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

/// Unit frame with all C++ frame-by-frame animations and the intermediate cycle.
///
/// `image_count` — the number of body sprite frames (the index bound);
/// `sentry_frames` — `Unit::GetFrameInfo().image_count` (`D_*`), i.e.
/// how many frames the animation block occupies. `RADAR` has 20 frames in the sprite, but
/// `D_RADAR.image_count = 16` (the last 4 are connectors), and C++
/// `image_index_max` counts by `FrameInfo` — so the animation must
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
/// **Building frames are not advanced by time** (DOS capture §7.2,
/// `max-original-dasm/formats/unit-animation.md`): factories have two state
/// frames, the visible blinking of a powered-on factory — palette cycles
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
    // `GetImageIndex()`. Explosions have no phases/rotations — the frame is taken directly.
    if unit.flags & unit_flags::EXPLODING != 0 {
        return usize::try_from(unit.image_index.max(0)).unwrap_or(0).min(last);
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

        // C++ `ProcessOrderSentry` (`units_manager.cpp:3029-3041`): start —
        // `image_base + angle` (from the `UnitInfo` constructor), then `+1` until
        // `image_index_max`, after the last frame — `image_base` again
        // (not `image_base + angle`). This is exactly
        // `base + ((angle + step) % image_count)`; for `RADAR` (angle = 0,
        // `D_RADAR.image_count = 16`) — 16 rotation frames.
        let phase = (usize::from(angle % 8) + animation.sentry_step as usize) % frames;
        index = base + phase;
    }

    index.min(last)
}

/// World point of the sprite anchor: the center of the visual cell, for buildings — +31.
///
/// C++ `UnitInfo::SetPosition` (`unitinfo.cpp:1301`) and `Render`
/// (`unitinfo.cpp:3138`) draw at `unit.x/unit.y`; for `BUILDING` this is the center of
/// the 2×2 rectangle, for the others — the cell center.
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
    /// — how many cells).
    pub resource_icons: u32,
    /// How many **cells** with the resource icon were drawn (`Survey_RenderMarkers`,
    /// `survey.cpp:123`): icons are visible only on surveyed cells.
    pub resource_icon_cells: u32,
    /// How many units were drawn with sprites.
    pub units_drawn: usize,
    /// How many units were drawn with markers (no sprite/assets).
    pub units_marker: usize,
    /// How many `BLDMRK*` construction markers were drawn (`DrawMap_RenderBuildMarker`).
    pub build_markers: usize,
    /// How many selection indicators were drawn (`GameManager_DrawUnitSelector`).
    pub selector_units: usize,
    /// How many units were skipped (not in the lists/not visible to the camera).
    pub units_skipped: usize,
    /// How many pixels of the 3×3 button overlays (GRID/HITS/AMMO/COLORS/STATUS/
    /// NAMES/RANGE/SCAN) were drawn — [`crate::overlays`].
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
    /// Body pixels (together with the shadow — it only darkens what is already
    /// drawn, so on an empty frame this is exactly the body).
    pub unit: u32,
    /// Turret/gun pixels; 0 — the type has no turret or flag.
    pub turret: u32,
    /// Connector pixels (`UnitInfo::RenderWithConnectors`): for a mine — 8
    /// frames of `D_MINING.connector_image_base = 16`, for `POWGEN` — 4 pipes
    /// `D_SMLBLD.connector_image_base = 2`. 0 — no connectors/no neighbors.
    pub connectors: u32,
}

/// Draws a unit with sprites: shadow → body → connectors → turret/gun.
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
/// Returns `None` if there is no loaded sprite/frame for the type — then the
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
    draw_unit_sprites_with_base(assets, unit, presentation, unit.image_base, frame, anchor, zoom)
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
/// `body_angle` — the direction with which the sprite **currently** moves on the map
/// (in the window loop this is `App::visual_angle`). The body takes the frame
/// `image_base + body_angle`, and the turret — its own `turret_image_base +
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

/// [`draw_unit_sprites_with_angle`] with the unit frame clock.
///
/// `animation` — [`FrameAnimation`] from [`crate::App::unit_animation`]:
/// the builder (`+16 → +24 → +32`), the `ANIMATED` building (`ProcessOrderSentry`) and
/// the **intermediate** frame cycle of a working factory. The HUD and tests pass
/// [`FrameAnimation::default()`] (the static C++ frame, as with `dt = 0`).
///
/// `effects` — `settings["effects"]`; `false` disables frame-by-frame animations,
/// as in C++.
///
/// The palette — the game system palette (`Assets::system_palette`); the map
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
        // `firing_image_base` (or 104+… for infantry) is held for the whole recoil.
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

    let shadow_offset = (
        i32::from(presentation.shadow_offset.x),
        i32::from(presentation.shadow_offset.y),
    );
    let mut stats = UnitSpriteStats::default();

    stats.unit = renderer
        .draw_unit_scaled(frame, sprite_frame, shadow, anchor, shadow_offset, zoom)
        .drawn;

    // Complex connectors — `UnitInfo::RenderWithConnectors`
    // (`unitinfo.cpp:3157`): each has its own frame `connector_image_base + n`,
    // drawn at the same anchor as the body (the frame hotspot defines
    // the side), the shadow — from the same `S_*` as the body.
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

    // Turret/gun — a separate frame over the body with the shift
    // `angle_offsets[body_angle]` (`UnitInfo::RenderWithConnectors`).
    if unit.flags & (unit_flags::TURRET_SPRITE | unit_flags::SPINNING_TURRET) != 0 {
        // During recoil the turret is drawn with the firing frame
        // (`DrawSpriteTurretFrame(turret_angle + firing_image_base)`).
        let firing_index = sprite
            .frame_info
            .as_ref()
            .and_then(|info| firing_turret_frame_index(info, unit))
            .filter(|index| *index < sprite.sheet.len());

        let turret = match firing_index {
            Some(index) => sprite.sheet.frame(index),
            None => sprite.turret_frame(unit.turret_angle),
        };
        let turret_shadow = match firing_index {
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

/// Team marker colors (fallback without sprites): the original team order —
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
/// `TIMER_FPS_TO_MS(24) * 5` = 42 ms × 5 = **210 ms**. The color is the palette index
/// `0xFF` (white) or `0x00` (black); `DrawUnitSelector` draws three nested
/// frames with alternation (`color ^= 0xFF` after each).
pub const UNIT_SELECTOR_BLINK_SECONDS: f32 = 0.210;

/// Palette for the selection/construction markers without assets: index `0xFF` — white,
/// `0x00` — black.
fn fallback_selector_palette() -> PaletteRgba {
    let mut palette = [[0u8, 0, 0, 255]; 256];
    palette[0xFF] = [255, 255, 255, 255];
    palette
}

/// Pixel segment with clipping by the buffer (only horizontal or
/// vertical — there are no other `draw_line` in `GameManager_DrawUnitSelector`).
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

/// Selected unit indicator — `GameManager_DrawUnitSelector`
/// (`game_manager.cpp:744-836`), called from `DrawMap_RenderUnit`
/// (`drawmap.cpp:813-833`).
///
/// `anchor` — the screen center of the sprite (`UnitInfo::x/y`, i.e. the cell center,
/// for `BUILDING` — `+31`, for the builder — with the tape shift). `is_building`
/// selects the corner length: C++ `size1` = 32 for a building and 16 for the others, the frame
/// step `size2` = 128/64, the anchor "half-size" — 64/32; everything is converted to
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

impl<D: Dispatcher> App<D> {
    /// Draws a frame of the current state into the `screen_width × screen_height` buffer.
    pub fn render_frame(&self) -> RgbaBuffer {
        self.render_frame_with_stats().0
    }

    /// The same as [`App::render_frame`], plus drawing statistics.
    ///
    /// If the window chrome (`FRAMEPIC`) is loaded and the frame size is 640×480, the
    /// 1:1 window is drawn as in the original: background panels from `MAX.RES`, and the map — in the
    /// [`max_render::layout::MAIN_MAP`] area (7×7 cells). Otherwise — the former
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

        // Surface: if the planet data is loaded — real 64×64 tiles from
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
        self.draw_units(&mut frame, self.viewport(), &mut stats);
        // 3×3 buttons of the left panel (`game_manager.cpp`): COLORS/STATUS/HITS/
        // AMMO/NAMES — over the sprites, RANGE/SCAN — circles over everything.
        self.draw_toggle_overlays(&mut frame, self.viewport(), &mut stats);
        // Resource icons — over units/buildings (`GameManager_RenderSurveyIndicator`).
        self.draw_resource_icons(&mut frame, self.viewport(), &mut stats);
        // Construction markers — last, like `DrawMap_RedrawDirtyZones`.
        self.draw_build_markers(&mut frame, self.viewport(), &mut stats);
        // Construction site ghost (`ORDER_STATE_SELECT_SITE`) — over the units.
        self.draw_build_site_ghost(&mut frame, self.viewport(), &mut stats);

        // `BuildMenu` in full-screen mode (without `FRAMEPIC`).
        self.draw_build_menu(&mut frame);
        // Save/load window (P3) — modal, like `AbstractMenu` in C++.
        self.draw_save_load(&mut frame);
        // Upgrades screen (P6).
        self.draw_upgrade_menu(&mut frame);
        // Research screen (P7) and the help window over it.
        self.draw_research_menu(&mut frame);
        // Mine extraction allocation dialog (`AllocMenu`, `ALLOCFRM`).
        self.draw_alloc_menu(&mut frame);
        // X-FER window (`TransferMenu`, `XFERPIC`) — over the map.
        self.draw_transfer_menu(&mut frame);
        // Selected unit command menu (`GameManager_PopupButtons`).
        self.draw_unit_menu(&mut frame);
        // `Preferences`/`Setup` dialog (`OptionsMenu`, `PREFSPIC`).
        self.draw_preferences(&mut frame);
        self.draw_help(&mut frame);
        // Building self-destruct dialog (UnitsManager_SelfDestructMenu).
        self.draw_self_destruct(&mut frame);
        // Pause dialog (PauseMenu_Menu -> DialogMenu, Alt+P) — over everything.
        self.draw_pause_dialog(&mut frame);

        if self.hud().hint.is_some() {
            let whole = PixelRect::new(0, 0, frame.width() as i32, frame.height() as i32);
            self.draw_status_message(&mut frame, whole);
        }

        (frame, stats)
    }

    /// `MESSAGE_BOX_INFO` message window above the map — `MessageManager_
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

        crate::panels::draw_status_message(frame, area, &palette, hint);
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
        // remain shared with the application. `map_viewport` — the same camera that
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
                stats.map_tiles = render_map_into(&mut map, &self.state_ref().map(), &viewport, &style);
            }
        }

        self.draw_grid_overlay(&mut map, &viewport, &mut stats);
        self.draw_units(&mut map, &viewport, &mut stats);
        // 3×3 buttons of the left panel: overlays over the sprites, like
        // `DrawMap_RenderUnitDisplays`/`GameManager_RenderScanRangeIndicators`.
        self.draw_toggle_overlays(&mut map, &viewport, &mut stats);
        // `GameManager_RenderSurveyIndicator` (`game_manager.cpp:991-995`) —
        // **after** the units, so the resource icons lie over buildings.
        self.draw_resource_icons(&mut map, &viewport, &mut stats);
        // Construction markers — around the selected builder/building
        // (`DrawMap_RenderBuildMarker`); C++ draws them in
        // `DrawMap_RedrawDirtyZones` last, over the resource icons.
        self.draw_build_markers(&mut map, &viewport, &mut stats);
        // Construction site ghost — into the map area, like `GameManager_TempTape`
        // (`ORDER_STATE_SELECT_SITE`), before the cell is confirmed.
        self.draw_build_site_ghost(&mut map, &viewport, &mut stats);
        // Route preview (the RMB green line) — over the units, like
        // `GameManager_DrawMapView`: in windowed mode `render_hud` is not
        // called (the panels are drawn by `FRAMEPIC`), so without this line
        // the preview was visible only in the full-screen fallback.
        if let Some(preview) = &self.hud.preview {
            crate::hud::draw_path_preview(&mut map, &viewport, preview);
        }
        crate::chrome::blit_map(&mut frame, &map);

        // Panels over the map: buttons, portrait, statistics, grid, zoom,
        // coordinates — like `GameManager_Draw*` in the original.
        if let Some(panels) = self.assets().and_then(|assets| assets.panels.as_ref()) {
            panels.draw(&mut frame, self);
        }

        // Selected builder hint — the `MESSAGE_BOX_INFO` message window
        // at the start of the map area (`MessageManager_DrawMessage`).
        self.draw_status_message(&mut frame, area);

        // `BuildMenu` — a modal screen over the frame (C++ `AbstractMenu`
        // closes the main menu: `GameManager_DisableMainMenu`).
        self.draw_build_menu(&mut frame);
        // Save/load window (P3).
        self.draw_save_load(&mut frame);
        // Upgrades screen (P6).
        self.draw_upgrade_menu(&mut frame);
        // Research screen (P7) and the help window over it.
        self.draw_research_menu(&mut frame);
        // Mine extraction allocation dialog — the full-screen `ALLOCFRM`.
        self.draw_alloc_menu(&mut frame);
        // X-FER window (`XFERPIC`) — over the map and panels.
        self.draw_transfer_menu(&mut frame);
        // Selected unit command menu — over the map and panels.
        self.draw_unit_menu(&mut frame);
        // `Preferences`/`Setup` dialog (`OptionsMenu`, `PREFSPIC`).
        self.draw_preferences(&mut frame);
        self.draw_help(&mut frame);
        // Building self-destruct dialog (UnitsManager_SelfDestructMenu).
        self.draw_self_destruct(&mut frame);
        // Pause dialog (PauseMenu_Menu -> DialogMenu, Alt+P) — over everything.
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
    /// The palette — the window palette (`FRAMEPIC`), like the other modal windows;
    /// if there is no chrome — the menu palette.
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

        menu.draw(frame, self.assets().and_then(|a| a.self_destruct.as_ref()), fallback);
    }

    /// Draws the pause dialog (`PauseMenu_Menu` → `DialogMenu`, `Alt+P`).
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
    /// `complex_grid_overlay` — the `settings` setting (`settings.cpp`), not ported
    /// in the port; the C++ default is taken (`0` — a simple grid
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

        stats.overlay_pixels +=
            crate::overlays::draw_grid(frame, self, viewport, &palette, false);
    }

    /// 3×3 button overlays (`DrawMap_RenderUnitDisplays`,
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

        stats.overlay_pixels += crate::overlays::draw_unit_overlays(frame, self, viewport, &palette);
        stats.overlay_pixels += crate::overlays::draw_range_scan(frame, self, viewport, &palette);
    }

    fn draw_units(
        &self,
        frame: &mut RgbaBuffer,
        viewport: &max_render::viewport::Viewport,
        stats: &mut RenderStats,
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
            // Visibility and drawing order — by the visual position: while
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
            if !unit.is_visible_to_team(self.state_ref().player_team()) {
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
        // (`GameManager_MarkerColor`). Without assets — white/black.
        let selector_palette = match self.assets() {
            Some(assets) => window_palette(assets),
            None => fallback_selector_palette(),
        };

        for index in draw_order(&positions) {
            let Some(unit) = self.state_ref().unit(visible[index]) else {
                continue;
            };
            // Visual position — a fractional cell walking the route
            // (see `App::update_visual_units`). The anchor — the cell center in world
            // pixels (for `BUILDING` buildings — +31, like `UnitInfo::SetPosition`),
            // then the camera converts it to screen coordinates.
            let (grid_x, grid_y) = self
                .visual_position(visible[index])
                .unwrap_or((f32::from(unit.grid.x), f32::from(unit.grid.y)));
            // Rocket/torpedo trail particles (`Paths_DrawMissile`) and the
            // projectiles themselves (`AirPath::Execute` — pixel step): C++ spreads
            // only the draw zones, the port keeps this in pixel `x/y`.
            let (world_x, world_y) = effect_pixel_anchor(unit)
                .or_else(|| missile_pixel_anchor(unit))
                .unwrap_or_else(|| unit_anchor_world(unit, (grid_x, grid_y)));
            // `UnitInfo::PositionInTape` + `OffsetDrawZones` (`unitinfo.cpp:4478`):
            // the constructor shifts by 8 px per tick toward the 2×2 tape center, 32 px in total
            // (half a cell) — to the center of the construction site. This is a frame
            // shift of the sprite, the builder grid does not change.
            let (tape_offset_x, tape_offset_y) =
                crate::buildsite::tape_center_offset(self.state_ref(), unit);
            let anchor = viewport.world_to_screen(world_x + tape_offset_x, world_y + tape_offset_y);

            let image_base = image_base_for(
                unit,
                self.state_ref()
                    .teams()
                    .get(unit.team.0 as usize)
                    .map(|team| team.team_clan),
            );
            // Body angle — of the current visual segment (`visual_angle`):
            // the simulation turns the unit at the route cell, while the sprite walks
            // it at the real speed; otherwise the frame would "run ahead" and the unit
            // would face sideways while moving straight.
            let body_angle = self.visual_angle(visible[index]).unwrap_or(unit.angle);
            // Frame-by-frame animation of the builder, `ANIMATED` buildings and the intermediate
            // full frame cycle of working mines/factories — the visual clock of
            // `App` (real `dt`), they are not part of the snapshot/CRC. `dt = 0`
            // (QA/headless) gives the static C++ frame (`+16` for the builder,
            // the base/powered-on building frame) — so the goldens do not drift.
            let animation = self.unit_animation(visible[index]);
            // Complex connectors (`UnitInfo::RenderWithConnectors`): the stored
            // mask from the save ∪ computed by neighbors (see [`drawn_connector_mask`]).
            let connectors = drawn_connector_mask(self.state_ref(), unit);
            // C++ `settings["effects"]` (`settings.cpp:128`): `effects = false`
            // disables frame-by-frame sprite animations (`UnitInfo::Animate`,
            // `units_manager.cpp:2857/3030`) — the frame is static.
            let effects = self.preferences.effects;
            let zoom = self.unit_render_zoom(visible[index], viewport.zoom());
            // Presentation fields (shadow/brightness) — a separate table
            // `GameState::presentation` (phase 5.3/B), not `UnitInfo`.
            let presentation = self
                .state_ref()
                .presentation(visible[index])
                .cloned()
                .unwrap_or_default();
            let drawn = self.assets().and_then(|assets| {
                // Frame palette: a snapshot with palette cycles or the original
                // system palette (`dt = 0` — no changes).
                let palette = self.animated_palette().unwrap_or(&assets.system_palette);
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
            // `DrawMap_RenderUnit` (`drawmap.cpp:813-833`): for the selected unit —
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
    /// sprite** without writing into the match state — the order is not issued before LMB, and
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

    /// `Access_IsSurveyorOverlayActive` (`access.cpp:341-351`): the selected
    /// unit — own team, type `SURVEYOR` and order **not** `ORDER_IDLE`.
    ///
    /// In C++ this is cached in `GameManager_IsSurveyorSelected` on unit selection
    /// (`GameManager_MenuUnitSelect`, `game_manager.cpp:7165`); the port computes
    /// the value from the current state — the same thing, but without a "stuck" flag
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
    /// The SURVEY button (`hud.panels.survey` — `GameManager_DisplayButtonSurvey`,
    /// `game_manager.cpp:3985`) and the selected scout enable showing resource
    /// icons; without them the icons are not drawn at all.
    pub fn survey_overlay_active(&self) -> bool {
        self.hud().panels.survey || self.selected_surveyor_active()
    }

    /// Current selection indicator color — `GameManager_MarkerColor`
    /// (`game_manager.cpp:222/1191`): `0xFF` (white) or `0x00` (black),
    /// toggled every [`UNIT_SELECTOR_BLINK_SECONDS`] (210 ms).
    pub fn marker_color_index(&self) -> u8 {
        self.marker_color_index
    }

    /// Resource icons — `DrawMap_RenderSurveyDisplay` (`drawmap.cpp:1065`) +
    /// `Survey_RenderMarkers` (`survey.cpp:123`).
    ///
    /// C++ (`game_manager.cpp:991-995`): `GameManager_RenderSurveyIndicator`
    /// calls the drawing only if `GameManager_DisplayButtonSurvey`
    /// (the panel **SURVEY** button) or `GameManager_IsSurveyorSelected`
    /// (`Access_IsSurveyorOverlayActive`, `access.cpp:341`: a selected
    /// `SURVEYOR` of own team not in `ORDER_IDLE`). Inside
    /// `Survey_RenderMarkers` the mask is the team `hash_team_id` (the `MAXSURVEY` cheat
    /// gives `0xFFFF` and is not enabled in the port), so the icon appears only on
    /// a **surveyed** cell (`Survey_SurveyArea` sets the bit when the scout
    /// passes by).
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
            // `GameManager_MaxSurvey` — a cheat (`CHEAT_CODE_MAXSURVEY`), not implemented
            // in the port: we show only surveyed cells.
            false,
        );

        stats.resource_icons += pixels;
        stats.resource_icon_cells += cells;
    }

    /// Blinking construction markers `BLDMRK1..5` — `DrawMap_RenderBuildMarker`
    /// (`drawmap.cpp:353`) + `DrawMap_RedrawUnit` (`drawmap.cpp:304`).
    ///
    /// Drawn when an own team unit is selected in `ORDER_IDLE` (a completed
    /// building waits for a manager) or a mobile builder in
    /// `ORDER_BUILD`/`ORDER_STATE_UNIT_READY` (completed and waiting for the order to leave
    /// the site): the symbols are placed on the cells around the **parent** of the selected
    /// unit (`Access_IsAccessible` filters occupied ones), and a click on such a cell —
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
            // 62×62 — the full 64 px cell frame, scale `zoom/64`.
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
/// STATIONARY)))`, the parent exists and it is not `AIRTRANS`. The start —
/// `(parent.x - 1, parent.y + 1)`, for a building `(+1 in y, limit = 3)`, then
/// four directions (`N, E, S, W` — `direction += 2`) for `limit` steps,
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

            if state.is_accessible(unit.unit_type,
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
/// without the chrome — `COLOR.PAL` as a reasonable fallback.
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
        // always — this is a debug map, not a game frame.
        (
            max_render::map_render::render_resources_into(frame, map, viewport, style),
            0,
        )
    }
}

/// 6×6 unit marker at the `anchor` point (cell center), scaled with
/// the map (`zoom`, 64 — 1:1).
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

/// Writes a [`RgbaBuffer`] into a binary PPM (`P6`) — viewing a frame without SDL.
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
mod tests {
    use super::*;
    use crate::AppConfig;

    #[test]
    fn frame_index_uses_the_given_body_angle_and_power_on_offset() {
        use max_core::enums::UnitType;
        use max_core::team::TeamId;
        use max_core::unit::{UnitId, UnitInfo};

        let mut unit = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
        unit.angle = 7;

        for angle in 0..8u8 {
            assert_eq!(
                unit_frame_index_for_angle(&unit, 10, 100, angle),
                10 + usize::from(angle),
                "кадр корпуса = image_base + angle"
            );
        }
        assert_eq!(unit_frame_index(&unit, 10, 100), 17);

        // `ORDER_POWER_ON` — the next frame of a powered-on building.
        unit.orders = UnitOrder::PowerOn;
        assert_eq!(unit_frame_index_for_angle(&unit, 0, 100, 2), 3);
        // The last frame does not overflow.
        assert_eq!(unit_frame_index_for_angle(&unit, 0, 3, 2), 2);
    }

    #[test]
    fn builder_work_cycle_matches_the_cpp_plus_eight_minus_sixteen_loop() {
        use max_core::enums::UnitType;
        use max_core::team::TeamId;
        use max_core::unit::{UnitId, UnitInfo};

        let mut builder = UnitInfo::new(UnitType::Constructor, TeamId::RED, UnitId(1));
        builder.orders = UnitOrder::Build;
        builder.state = UnitOrderState::BuildInProgress;

        // CONSTRCT: 40 frames, angle 3 → image_index = 3 + 16 = 19.
        // C++ ProcessOrderBuild: +8, +8, then 35 + 8 >= 40 → 35 - 16 = 19.
        let cycle: Vec<usize> = (0..7)
            .map(|step| unit_frame_index_for_animation(&builder, 0, 40, 3, step % 3))
            .collect();
        assert_eq!(cycle, vec![19, 27, 35, 19, 27, 35, 19]);

        // Angle 0: construction start frames +16 = 16, then 24 and 32.
        let cycle: Vec<usize> = (0..3)
            .map(|step| unit_frame_index_for_animation(&builder, 0, 40, 0, step))
            .collect();
        assert_eq!(cycle, vec![16, 24, 32]);

        // The engineer has no cycle: the working pose +16 is constant (StartBuilding).
        let engineer = {
            let mut unit = UnitInfo::new(UnitType::Engineer, TeamId::RED, UnitId(2));
            unit.orders = UnitOrder::Build;
            unit.state = UnitOrderState::BuildInProgress;
            unit
        };
        for step in 0..3 {
            assert_eq!(
                unit_frame_index_for_animation(&engineer, 0, 32, 3, step),
                19,
                "ENGINEER держит DrawSpriteFrame(image_index + 16)"
            );
        }

        // After construction the frame returns to the base regardless of the step.
        builder.orders = UnitOrder::Await;
        builder.state = UnitOrderState::ExecutingOrder;
        for step in 0..3 {
            assert_eq!(unit_frame_index_for_animation(&builder, 0, 40, 3, step), 3);
        }
    }

    #[test]
    fn scaler_scale_matches_the_cpp_divisor() {
        assert_eq!(scaler_scale(0), 1.0);
        assert_eq!(scaler_scale(1), 0.5);
        assert_eq!(scaler_scale(2), 0.25);
        assert_eq!(scaler_scale(3), 0.125);
        assert_eq!(scaler_scale(4), 0.0625);
        // `scaler_adjust` in C++ — uint8_t, but really 0..4; we clamp it.
        assert_eq!(scaler_scale(9), 0.0625);
    }

    #[test]
    fn frame_without_assets_has_screen_size_and_is_opaque() {
        let app = App::new(AppConfig::default().with_screen(64, 48));
        let (frame, stats) = app.render_frame_with_stats();

        assert_eq!(frame.width(), 64);
        assert_eq!(frame.height(), 48);
        assert_eq!(frame.len(), 64 * 48);
        assert!(stats.map_tiles > 0, "карта должна рисоваться без ассетов");
        assert_eq!(stats.units(), stats.units_drawn + stats.units_marker);
        assert!(frame.pixels().chunks_exact(4).all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn enemy_units_are_rendered_only_when_visible() {
        use max_core::enums::UnitType;
        use max_core::geo::Point;
        use max_core::team::TeamId;

        let mut app = App::new(AppConfig::default());
        app.universe = max_core::universe::GameUniverse::default();

        let own = app.universe.spawn_unit(UnitType::Tank, TeamId::RED, Point::new(55, 55), 0)
            .expect("свой танк");
        let enemy = app.universe.spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(56, 55), 0)
            .expect("чужой танк");

        assert!(app
            .state_ref()
            .unit(own)
            .unwrap()
            .is_visible_to_team(TeamId::RED));
        assert!(
            !app.state_ref()
                .unit(enemy)
                .unwrap()
                .is_visible_to_team(TeamId::RED),
            "чужой юнит вне FOV не виден"
        );

        // Without assets units are drawn with markers: own — one.
        let (frame, stats) = app.render_frame_with_stats();
        assert_eq!(stats.units_marker, 1, "виден только свой юнит");
        assert_eq!(stats.units_drawn, 0);
        assert_eq!(
            frame.get(
                55 * 64 + 32 - app.viewport().origin().0,
                55 * 64 + 32 - app.viewport().origin().1
            ),
            Some(MARKER_COLORS[0]),
            "свой маркер на карте"
        );

        // Appeared in the FOV (survey/radar) — a foreign one is drawn too.
        app.universe
            .unit_mut(enemy)
            .unwrap()
            .set_visible_to_team(TeamId::RED, true);
        let (_, stats) = app.render_frame_with_stats();
        assert_eq!(stats.units_marker, 2, "видимый чужой юнит рисуется");
    }

    #[test]
    fn ppm_header_is_p6_with_frame_dimensions() {
        let path = std::env::temp_dir().join("max_app_render_test.ppm");
        let frame = RgbaBuffer::filled(3, 2, Rgba::rgb(1, 2, 3));

        write_ppm(&path, &frame).expect("ppm written");
        let bytes = std::fs::read(&path).expect("ppm read back");
        let header = b"P6\n3 2\n255\n";

        assert_eq!(&bytes[..header.len()], header);
        assert_eq!(bytes.len(), header.len() + 3 * 2 * 3);
        assert_eq!(&bytes[header.len()..header.len() + 3], &[1, 2, 3]);

        let _ = std::fs::remove_file(&path);
    }
}
