//! 3×3 button overlays in the left panel — 1:1 per the C++ oracle.
//!
//! Each button of the row enables its own map rendering layer (`game_manager.cpp`):
//!
//! | Button | C++ flag | Where it is drawn |
//! |---|---|---|
//! | SURVEY | `GameManager_DisplayButtonSurvey` | `GameManager_RenderSurveyIndicator` → `DrawMap_RenderSurveyDisplay` (`survey.cpp`) — already ported in [`crate::render`] |
//! | STATUS | `GameManager_DisplayButtonStatus` | `DrawMap_RenderUnitDisplays` → `DrawMap_RenderStatusDisplay` (`drawmap.cpp:444`): `IL_DSBLD`/`IL_SPEED`/`IL_SHOTS` icons |
//! | COLORS | `GameManager_DisplayButtonColors` | `DrawMap_RenderUnitDisplays` (`drawmap.cpp:581`): team color frame around **non**-stationary units |
//! | HITS | `GameManager_DisplayButtonHits` | `DrawMap_RenderUnitDisplays` (`drawmap.cpp:605-669`): a `hits` bar under the unit |
//! | AMMO | `GameManager_DisplayButtonAmmo` | the same place: a second `ammo` bar only for own team |
//! | RANGE | `GameManager_DisplayButtonRange` | `GameManager_RenderScanRangeIndicators` (`game_manager.cpp:947`): a circle of radius `ATTRIB_RANGE` |
//! | SCAN | `GameManager_DisplayButtonScan` | the same place (`:956`): a circle of radius `ATTRIB_SCAN` |
//! | GRID | `GameManager_DisplayButtonGrid` | `DrawMap_RenderMapTiles(..., display_button_grid)` → `DrawMap_RenderSimpleGridOverlay` / `DrawMap_RenderComplexGridOverlay` (`drawmap.cpp:922/956`) |
//! | NAME | `GameManager_DisplayButtonNames` | `DrawMap_RenderNamesDisplay` (`drawmap.cpp:747`): the unit name above it |
//!
//! `DrawMap_RenderNamesDisplay` also has two branches that are **not** part of the
//! NAME button: the finished-research text and the blinking green
//! «New … available» label of a factory/builder that waits for a player action
//! ([`unit_ready_label`], palette slot `0x1F` with the blinking green cycle).
//!
//! All flags are regular `CTInfo::display_button_*` toggles
//! (`GameManager_MenuClick*Button`, `game_manager.cpp:3800-3990`): a press
//! changes the flag and toggles the `*_ON` picture (`SetRestState`), and
//! `UpdateDrawBounds` redraws the map. In the port the flags live in
//! [`crate::panels::PanelState`], a button click already toggles them
//! ([`crate::ui`]) — here there is only drawing.

use max_core::enums::{unit_flags, Surface, UnitOrder, UnitOrderState, UnitType};
use max_core::unit::UnitInfo;
use max_core::universe::StateRef;
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::text::{draw_text_gnw_shadowed, GnwFont, GnwFontId};
use max_render::viewport::{Viewport, MAP_TILE_PIXELS};

use crate::panels::UiPalette;
use crate::App;
use max_core::universe::Dispatcher;

/// Window palette indices (`color.h:36-43`).
pub const COLOR_BLACK: u8 = 0x00;
/// `COLOR_RED`.
pub const COLOR_RED: u8 = 0x01;
/// `COLOR_GREEN`.
pub const COLOR_GREEN: u8 = 0x02;
/// `COLOR_BLUE`.
pub const COLOR_BLUE: u8 = 0x03;
/// `COLOR_YELLOW`.
pub const COLOR_YELLOW: u8 = 0x04;
/// `COLOR_CHROME_YELLOW`.
pub const COLOR_CHROME_YELLOW: u8 = 0x05;
/// «derelict» color (`drawmap.cpp:595`).
pub const COLOR_DERELICT: u8 = 0xA9;
/// Gray team color (`drawmap.cpp:592`).
pub const COLOR_GRAY: u8 = 0xFF;

/// Grid colors by surface type — `World::Init`
/// (`world.cpp:536-539`): RGB **divided by 4**, then `Color_MapColor`.
pub const GRID_RGB_LAND: [u8; 3] = [49 / 4, 97 / 4, 8 / 4];
/// Water grid (`world.cpp:537`).
pub const GRID_RGB_WATER: [u8; 3] = [58 / 4, 81 / 4, 173 / 4];
/// Coast grid (`world.cpp:538`).
pub const GRID_RGB_COAST: [u8; 3] = [82 / 4, 158 / 4, 20 / 4];
/// Air grid (`world.cpp:539`).
pub const GRID_RGB_AIR: [u8; 3] = [206 / 4, 32 / 4, 0 / 4];

/// `World::GetGridOverlayColor(surface_type)` (`world.cpp:1040`).
pub fn grid_color(palette: &UiPalette, surface: Surface) -> Rgba {
    let rgb = if surface.contains(Surface::AIR) {
        GRID_RGB_AIR
    } else if surface.contains(Surface::COAST) {
        GRID_RGB_COAST
    } else if surface.contains(Surface::WATER) {
        GRID_RGB_WATER
    } else {
        GRID_RGB_LAND
    };

    palette.map_color(rgb)
}

/// Screen rectangle of a unit — as in `DrawMap_RenderUnitDisplays`
/// (`drawmap.cpp:543-577`).
///
/// C++ computes from `unit->x/y` (sprite center in world pixels) and
/// `unit_size` 64/32 world px, then converts to screen:
/// `screen = (world * 65536) / scaling = world * zoom / 64`.
fn unit_screen_rect(unit: &UnitInfo, visual: (f32, f32), viewport: &Viewport) -> (PixelRect, i32) {
    let building = unit.is_quad();
    let world_size = if building { 64i32 } else { 32i32 };
    let (world_x, world_y) = crate::render::unit_anchor_world(unit, visual);
    let zoom = viewport.zoom() as i32;
    let (ulx, uly) = viewport.world_to_screen(world_x - world_size, world_y - world_size);
    // `unit_size = (unit_size * 2 * DEN) / scaling - 2`.
    let size = (2 * world_size) * zoom / i32::from(MAP_TILE_PIXELS) - 2;

    (PixelRect::new(ulx, uly, size, size), size)
}

/// `DrawMap_RenderColorsDisplay` (`drawmap.cpp:511`): a 1 px frame of four
/// lines along the unit bounds.
fn draw_color_frame(frame: &mut RgbaBuffer, rect: PixelRect, color: Rgba) -> u32 {
    let mut painted = 0u32;

    for x in rect.ulx..=rect.lrx() {
        painted += u32::from(frame.set(x, rect.uly, color));
        painted += u32::from(frame.set(x, rect.lry(), color));
    }

    for y in rect.uly..=rect.lry() {
        painted += u32::from(frame.set(rect.ulx, y, color));
        painted += u32::from(frame.set(rect.lrx(), y, color));
    }

    painted
}

/// The `COLOR_YELLOW` multi-selection frame around one group member —
/// `DrawMap_RenderUnit` (`drawmap.cpp:802-806`) →
/// `DrawMap_RenderColorsDisplay(unit)` (`drawmap.cpp:674-690`).
///
/// The oracle frame is anchored on `unit->x - 32` (the sprite center minus half a
/// cell) with `size = Gfx_ZoomLevel - 2`, i.e. a full 1-px rectangle around the
/// unit — **not** four corner brackets. A member of the selected unit's group gets
/// it while the group list is shared with `GameManager_SelectedUnit`.
pub fn draw_group_member_frame(
    frame: &mut RgbaBuffer,
    viewport: &Viewport,
    unit: &UnitInfo,
    visual: (f32, f32),
    color: Rgba,
) -> u32 {
    let (world_x, world_y) = crate::render::unit_anchor_world(unit, visual);
    let (screen_x, screen_y) = viewport.world_to_screen(world_x - 32, world_y - 32);
    let ulx = screen_x + 1;
    let uly = screen_y + 1;
    let size = viewport.zoom() as i32 - 2;

    if ulx + size < 0 || uly + size < 0 {
        return 0;
    }
    if ulx >= frame.width() as i32 || uly >= frame.height() as i32 {
        return 0;
    }

    draw_color_frame(
        frame,
        PixelRect::new(ulx, uly, size.max(0), size.max(0)),
        color,
    )
}

/// `GameManager_RenderMultiSelectIndicator` (`game_manager.cpp:997-1035`): the
/// yellow frame of the rectangle selection.
///
/// The bounds are in map pixels (`GameManager_MultiSelectBounds`); the oracle draws
/// each of the four lines only when that edge falls inside the map window and clips
/// the line itself to the window (`map_ulx2..map_lrx2`, `:1013-1016`), which the
/// buffer write does here.
pub fn draw_multi_select_frame(
    frame: &mut RgbaBuffer,
    viewport: &Viewport,
    upper: (i32, i32),
    lower: (i32, i32),
    color: Rgba,
) -> u32 {
    let (ulx, uly) = viewport.world_to_screen(upper.0, upper.1);
    let (lrx, lry) = viewport.world_to_screen(lower.0, lower.1);
    let width = frame.width() as i32;
    let height = frame.height() as i32;
    let mut painted = 0u32;

    let clamp_x = |x: i32| x.clamp(0, width - 1);
    let clamp_y = |y: i32| y.clamp(0, height - 1);

    // The oracle enters the drawing only when the selection rectangle intersects
    // the map window at all (`:1009-1012`).
    if ulx > width - 1 || lrx < 0 || uly > height - 1 || lry < 0 {
        return 0;
    }

    // Top edge (`:1018-1021`) — only when its own row is inside the window.
    if uly >= 0 && uly < height {
        for x in clamp_x(ulx)..=clamp_x(lrx) {
            painted += u32::from(frame.set(x, uly, color));
        }
    }

    // Right edge (`:1022-1025`).
    if lrx >= 0 && lrx < width {
        for y in clamp_y(uly)..=clamp_y(lry) {
            painted += u32::from(frame.set(lrx, y, color));
        }
    }

    // Bottom edge (`:1026-1029`).
    if lry >= 0 && lry < height {
        for x in clamp_x(ulx)..=clamp_x(lrx) {
            painted += u32::from(frame.set(x, lry, color));
        }
    }

    // Left edge (`:1030-1033`).
    if ulx >= 0 && ulx < width {
        for y in clamp_y(uly)..=clamp_y(lry) {
            painted += u32::from(frame.set(ulx, y, color));
        }
    }

    painted
}

/// Palette index of the team color by its slot — `TeamUnits::hash_team_id`
/// (`resource_manager.cpp:969-981`).
///
/// C++ stores the team color as a bit in `unit->flags` (`UnitInfo::UnitInfo`,
/// `unitinfo.cpp:179`), and `DrawMap_RenderUnitDisplays` (`drawmap.cpp:581-598`)
/// picks `COLOR_RED/GREEN/BLUE/0xFF` from it. The port does not write the same bits at
/// spawn (the flag set is part of the reference digest) and restores the color with the
/// same slot→color mapping from `unit.team`
/// ([`max_core::team::hash_team_id`]); bits from saves are honoured directly.
pub const fn team_color_index(team: max_core::team::TeamId) -> u8 {
    let hash = max_core::team::hash_team_id(team);

    if hash == unit_flags::HASH_TEAM_RED as u16 {
        COLOR_RED
    } else if hash == unit_flags::HASH_TEAM_GREEN as u16 {
        COLOR_GREEN
    } else if hash == unit_flags::HASH_TEAM_BLUE as u16 {
        COLOR_BLUE
    } else if hash == unit_flags::HASH_TEAM_GRAY as u16 {
        COLOR_GRAY
    } else {
        COLOR_DERELICT
    }
}

/// `COLORS` frame color by the team flags (`drawmap.cpp:581-598`), as in C++.
///
/// `HASH_TEAM_*` bits are the **team** color (RED/GREEN/BLUE/GRAY slot), not
/// `team_clan`: in C++ `team_clan` defines only the mine clan frames
/// (`unitinfo.cpp:282`) and trade modifiers, it has nothing to do with the frame
/// color. If there are no bits in `flags` (new port units, synthetic tests) —
/// the same `unit.team → color` mapping is taken ([`team_color_index`]); a unit
/// without a team (ALIEN/derelict) is drawn with `COLOR_DERELICT`.
pub fn team_frame_color(palette: &UiPalette, unit: &UnitInfo) -> Rgba {
    let index = if unit.flags & unit_flags::HASH_TEAM_RED != 0 {
        COLOR_RED
    } else if unit.flags & unit_flags::HASH_TEAM_GREEN != 0 {
        COLOR_GREEN
    } else if unit.flags & unit_flags::HASH_TEAM_BLUE != 0 {
        COLOR_BLUE
    } else if unit.flags & unit_flags::HASH_TEAM_GRAY != 0 {
        COLOR_GRAY
    } else if unit.flags & unit_flags::HASH_TEAM_DERELICT != 0 {
        COLOR_DERELICT
    } else {
        team_color_index(unit.team)
    };

    palette.rgba(index)
}

/// `Gfx_RenderCircle` (`gfx.cpp:536`): a circle outline over eight octants.
fn draw_circle(frame: &mut RgbaBuffer, xc: i32, yc: i32, radius: i32, color: Rgba) -> u32 {
    if radius <= 0 {
        return 0;
    }

    let mut painted = 0u32;

    fn plot(frame: &mut RgbaBuffer, x: i32, y: i32, color: Rgba, painted: &mut u32) {
        if frame.set(x, y, color) {
            *painted += 1;
        }
    }

    let mut x = 0;
    let mut y = radius;
    let mut d = 3 + radius * -2;

    plot(frame, x + xc, y + yc, color, &mut painted);
    plot(frame, x + xc, -y + yc, color, &mut painted);
    plot(frame, -x + xc, -y + yc, color, &mut painted);
    plot(frame, -x + xc, y + yc, color, &mut painted);
    plot(frame, y + xc, x + yc, color, &mut painted);
    plot(frame, y + xc, -x + yc, color, &mut painted);
    plot(frame, -y + xc, -x + yc, color, &mut painted);
    plot(frame, -y + xc, x + yc, color, &mut painted);

    while x <= y {
        if d <= 0 {
            d += 4 * x + 6;
        } else {
            d += 4 * (x - y) + 10;
            y -= 1;
        }

        x += 1;

        plot(frame, x + xc, y + yc, color, &mut painted);
        plot(frame, x + xc, -y + yc, color, &mut painted);
        plot(frame, -x + xc, -y + yc, color, &mut painted);
        plot(frame, -x + xc, y + yc, color, &mut painted);
        plot(frame, y + xc, x + yc, color, &mut painted);
        plot(frame, y + xc, -x + yc, color, &mut painted);
        plot(frame, -y + xc, -x + yc, color, &mut painted);
        plot(frame, -y + xc, x + yc, color, &mut painted);
    }

    painted
}

/// `DrawMap_RenderBarDisplay` (`drawmap.cpp:429`) — rectangle fill.
fn fill_bar(frame: &mut RgbaBuffer, ulx: i32, uly: i32, lrx: i32, lry: i32, color: Rgba) -> u32 {
    let width = lrx - ulx + 1;
    let height = lry - uly + 1;

    if width <= 0 || height <= 0 {
        return 0;
    }

    frame.fill_rect(PixelRect::new(ulx, uly, width, height), color)
}

/// Copies a `SimpleImage` with transparency (the `image.transparent` index).
fn blit_simple(
    frame: &mut RgbaBuffer,
    image: &max_assets::image::SimpleImage,
    ulx: i32,
    uly: i32,
    palette: &UiPalette,
) -> u32 {
    let mut painted = 0u32;

    for y in 0..u32::from(image.height.max(0) as u16) {
        for x in 0..u32::from(image.width.max(0) as u16) {
            let Some(index) = image.pixel(x, y) else {
                continue;
            };

            if index == image.transparent {
                continue;
            }

            if frame.set(ulx + x as i32, uly + y as i32, palette.rgba(index)) {
                painted += 1;
            }
        }
    }

    painted
}

/// `DrawMap_RenderStatusDisplay` (`drawmap.cpp:444-509`): status icons.
fn draw_status_icons(
    frame: &mut RgbaBuffer,
    rect: PixelRect,
    unit: &UnitInfo,
    palette: &UiPalette,
    icons: &crate::panels::StatusIcons,
) -> u32 {
    // C++ calls `DrawMap_RenderStatusDisplay(unit, ulx - 1, uly - 1, lrx + 2, lry + 2)`.
    let ulx = rect.ulx - 1;
    let uly = rect.uly - 1;
    let mut width = rect.lrx() + 2;
    let height = rect.lry() + 2;
    let mut painted = 0u32;

    if unit.orders == UnitOrder::Disable {
        let Some(image) = icons.disabled.as_ref() else {
            return 0;
        };

        if i32::from(image.width) <= width - ulx && i32::from(image.height) <= height - uly {
            painted += blit_simple(
                frame,
                image,
                (width + ulx - i32::from(image.width)) / 2,
                (height + uly - i32::from(image.height)) / 2,
                palette,
            );
        }

        return painted;
    }

    if unit.speed == 0 && unit.shots == 0 {
        return 0;
    }

    width -= ulx;

    let speed = if unit.speed != 0 && icons.speed.is_some() {
        icons.speed.as_ref()
    } else {
        None
    };

    if let Some(image) = speed {
        if i32::from(image.height) > height - uly {
            return 0;
        }

        width -= i32::from(image.width);
    }

    let show_shots = unit.shots > 0 && width >= 0;
    let shots = if show_shots && icons.shots.is_some() {
        icons.shots.as_ref()
    } else {
        None
    };

    if let Some(image) = shots {
        if i32::from(image.height) > height - uly {
            return 0;
        }

        width -= i32::from(image.width);
    }

    if width < 0 {
        return 0;
    }

    if shots.is_some() && speed.is_some() {
        width /= 3;
    } else {
        width /= 2;
    }

    if let Some(image) = speed {
        painted += blit_simple(
            frame,
            image,
            width + ulx,
            height - i32::from(image.height) - 1,
            palette,
        );

        width = width * 2 + i32::from(image.width);
    }

    if let Some(image) = shots {
        painted += blit_simple(
            frame,
            image,
            width + ulx,
            height - i32::from(image.height) - 1,
            palette,
        );
    }

    painted
}

/// `DrawMap_RenderUnitDisplays` (`drawmap.cpp:605-669`): two bars —
/// `hits` and `ammo` — under the unit.
#[allow(clippy::too_many_arguments)]
fn draw_hit_ammo_bars(
    frame: &mut RgbaBuffer,
    rect: PixelRect,
    size: i32,
    unit: &UnitInfo,
    palette: &UiPalette,
    player_team: max_core::team::TeamId,
    zoom: i32,
    hits_enabled: bool,
    ammo_enabled: bool,
) -> u32 {
    let mut display_size = size / 8;

    if display_size < 3 {
        display_size = 3;
    }

    // `unit_size -= 0x100000 / Gfx_MapScalingFactor` = zoom / 4;
    // `ulx += 0x80000 / scaling` = zoom / 8; `uly += 0x60000 / scaling` = 3·zoom/8.
    let bar_width = size - zoom / 4;
    let bar_ulx = rect.ulx + zoom / 8;
    let mut bar_uly = rect.uly + 3 * zoom / 8;
    let mut painted = 0u32;

    for index in 0..2 {
        let (value_max, value_actual) = if index == 0 {
            if hits_enabled {
                let max = unit
                    .base_values
                    .get_attribute(max_core::unit_values::ATTRIB_HITS);
                (max, i32::from(unit.hits).min(max))
            } else {
                (0, 0)
            }
        } else if ammo_enabled && unit.team == player_team {
            let max = unit
                .base_values
                .get_attribute(max_core::unit_values::ATTRIB_AMMO);
            (max, i32::from(unit.ammo).min(max))
        } else {
            (0, 0)
        };

        if value_max > 0 {
            painted += fill_bar(
                frame,
                bar_ulx,
                bar_uly,
                bar_ulx + bar_width - 1,
                bar_uly + display_size - 1,
                palette.rgba(COLOR_BLACK),
            );

            let width = ((bar_width - 2) * value_actual) / value_max;

            if width > 0 {
                let color = if value_actual > value_max / 4 {
                    if value_actual > value_max / 2 {
                        COLOR_GREEN
                    } else {
                        COLOR_YELLOW
                    }
                } else {
                    COLOR_RED
                };

                painted += fill_bar(
                    frame,
                    bar_ulx + 1,
                    bar_uly + 1,
                    bar_ulx + width,
                    bar_uly + display_size - 2,
                    palette.rgba(color),
                );
            }
        }

        bar_uly += display_size - 1;
    }

    painted
}

/// `DrawMap_RenderNamesDisplay` + `DrawMap_RenderTextBox` (`drawmap.cpp:747-777`).
///
/// C++ draws three kinds of text over a unit of the local team, in this order:
///
/// 1. a powered-on `RESEARCH` whose topic is finished — `_(e3a4)`
///    (`%s research completed`);
/// 2. a unit in `ORDER_STATE_UNIT_READY` — `DrawMap_UnitCompletionLabels`
///    (`_(d7d0)/(2ec0)/(7c23)`, one per grammatical gender, all
///    `New %s available` in en-US). This is the original's "the factory has
///    produced a unit / the builder has finished the building" marker; for a
///    stationary parent (`parent->flags & STATIONARY`) the text moves to the
///    parent, i.e. onto the finished building, and for a factory (the parent is
///    the produced unit stored inside it) it stays on the factory itself;
/// 3. `GameManager_DisplayButtonNames` — the unit name in `COLOR_YELLOW`
///    (`DrawMap_RenderTextBox(unit, text, GNW_TEXT_OUTLINE | COLOR_YELLOW)`).
///
/// Branches 1–2 use `GNW_TEXT_OUTLINE | 0x1F` — palette slot 31, whose **green**
/// channel is the blinking palette cycle (see [`ready_label_color`]) — and are
/// **not** subject to the NAME toggle: C++ draws them for every own unit of the
/// draw group, which is why [`draw_unit_overlays`] no longer skips them when the
/// 3×3 panel buttons are off.
fn draw_unit_label<D: Dispatcher>(
    frame: &mut RgbaBuffer,
    app: &App<D>,
    viewport: &Viewport,
    palette: &UiPalette,
    unit: &UnitInfo,
    rect: PixelRect,
    zoom: i32,
) -> u32 {
    if unit.team != app.state_ref().player_team() {
        return 0;
    }

    if let Some((text, target)) = unit_ready_label(app.state_ref(), unit) {
        // `if (parent->flags & STATIONARY) unit = parent;` — the label goes over the
        // finished building (the builder waits on the site); a factory keeps it on
        // itself, because there the parent is the mobile unit held inside.
        let (target_rect, target_building) = if target == unit.id {
            (rect, unit.is_quad())
        } else {
            let Some(parent) = app.state_ref().unit(target) else {
                return 0;
            };
            let visual = app
                .visual_position(target)
                .unwrap_or((f32::from(parent.grid.x), f32::from(parent.grid.y)));

            (
                unit_screen_rect(parent, visual, viewport).0,
                parent.is_quad(),
            )
        };

        return draw_map_label(
            frame,
            &text,
            target_rect,
            zoom,
            target_building,
            ready_label_color(app),
            palette,
        );
    }

    // `GameManager_DisplayButtonNames && (unit->flags & SELECTABLE) &&
    // unit->GetUnitType() != LRGTAPE && != SMLTAPE`.
    if !app.hud().panels.names
        || unit.flags & unit_flags::SELECTABLE == 0
        || matches!(unit.unit_type, UnitType::LargeTape | UnitType::SmallTape)
    {
        return 0;
    }

    // `unit->GetDisplayName` (`drawmap.cpp:774`): `"Mk II Scout 7"`.
    let text = crate::panels::unit_display_name(unit);

    draw_map_label(
        frame,
        &text,
        rect,
        zoom,
        unit.is_quad(),
        palette.rgba(COLOR_YELLOW),
        palette,
    )
}

/// The completion label of `DrawMap_RenderNamesDisplay` (`drawmap.cpp:747-777`):
/// the text and the unit it is drawn over.
///
/// `Some` for the two branches that are not the regular NAME display:
/// 1. a powered-on `RESEARCH` whose topic is finished — `_(e3a4)`, drawn over the
///    research center;
/// 2. `ORDER_STATE_UNIT_READY` — `DrawMap_UnitCompletionLabels` over the parent when
///    it is stationary (the finished building) and over the unit otherwise (the
///    factory that holds the produced unit).
///
/// Both are independent of the NAME toggle in C++ (`DrawMap_RenderNamesDisplay` is
/// called for every own unit of the draw group).
pub fn unit_ready_label(
    state: StateRef<'_>,
    unit: &UnitInfo,
) -> Option<(String, max_core::unit::UnitId)> {
    if let Some(text) = research_completed_label(state, unit) {
        return Some((text, unit.id));
    }

    if unit.state != UnitOrderState::UnitReady {
        return None;
    }

    let parent = unit.parent_unit.and_then(|id| state.unit(id))?;
    let text = NEW_UNIT_AVAILABLE.replace("{}", crate::buildmenu::unit_name(parent.unit_type));
    let target = if parent.is_mobile() {
        unit.id
    } else {
        parent.id
    };

    Some((text, target))
}

/// `DrawMap_RenderTextBox` (`drawmap.cpp:692-745`) — the text box of a unit.
///
/// `text_area` = the unit bounds inset by 4 px; the font depends on the zoom
/// (`Gfx_ZoomLevel`, for non-buildings `/ 2`): `>= 42` — `FONT_1`, `>= 24` —
/// `FONT_5`, otherwise `FONT_2`.
///
/// `Text_AutofitTextBox(..., horizontal_align = true)` → `Text_TextBox`: the text is
/// split into rows by the box width (`Text_SplitText`, `text.cpp:586`), at most
/// `height / (font_height + 1)` rows, centred as a block, every row centred; a row
/// still wider than the box starts at the left edge and is clipped by
/// `Text_Blit`'s `swidth` (`Text_AutofitTextBox` clips to the visible map area, not
/// to the font). `Text_SplitText` also hyphenates a single word wider than the box —
/// the port leaves such a word on its own row and clips it; the completion labels
/// and unit names consist of shorter words, for which both paths are identical.
fn draw_map_label(
    frame: &mut RgbaBuffer,
    text: &str,
    rect: PixelRect,
    zoom: i32,
    building: bool,
    color: Rgba,
    palette: &UiPalette,
) -> u32 {
    let text_area = PixelRect::new(
        rect.ulx + 4,
        rect.uly + 4,
        (rect.width - 6).max(1),
        (rect.height - 6).max(1),
    );

    if text.is_empty() {
        return 0;
    }

    let zoom_level = if building { zoom } else { zoom / 2 };
    let font = if zoom_level >= 42 {
        GnwFontId::Font1.font()
    } else if zoom_level >= 24 {
        GnwFontId::Font5.font()
    } else {
        GnwFontId::Font2.font()
    };

    // `Text_TextBox`: `font_height = Text_GetHeight() + 1` with `GNW_TEXT_OUTLINE`.
    let font_height = font.height() + 1;
    let max_rows = (text_area.height / font_height).max(1) as usize;
    let rows = wrap_label_rows(text, font, text_area.width, max_rows);

    if rows.is_empty() {
        return 0;
    }

    // `vertical_align = true` (`Text_TextBox` default, used by `Text_AutofitTextBox`).
    let mut y = text_area.uly + ((text_area.height + 1) - font_height * rows.len() as i32) / 2;
    let mut painted = 0u32;

    for row in &rows {
        let width = font.text_width(row);

        if width > text_area.width {
            painted += draw_clipped_row(
                frame,
                text_area.ulx,
                y,
                text_area,
                font,
                row,
                color,
                palette,
            );
        } else if !row.is_empty() {
            let x = text_area.ulx + (text_area.width - width) / 2;

            painted += draw_text_gnw_shadowed(
                frame,
                x,
                y,
                font,
                row,
                color,
                palette.rgba(COLOR_BLACK),
                palette.rgba(COLOR_BLACK),
            )
            .max(0) as u32;
        }

        y += font_height;
    }

    painted
}

/// Rows of `Text_SplitText` (`text.cpp:586-698`) for the unit text box.
///
/// Greedy word wrap by the box width: a word goes onto the current row while
/// `row_width + space + word_width <= width`, a `\n` is a hard break, and once
/// `max_rows` rows exist the rest is appended to the last one (`Text_SplitText`
/// does the same when `row_count` reached `max_row_count`).
fn wrap_label_rows(text: &str, font: &GnwFont, width: i32, max_rows: usize) -> Vec<String> {
    let max_rows = max_rows.max(1);
    let space = font.char_width(' ');
    let mut rows: Vec<String> = Vec::new();
    let mut current = String::new();

    /// Starts a new row, or appends to the last one when no rows are left.
    fn break_row(rows: &mut Vec<String>, current: &mut String, max_rows: usize) {
        if rows.len() < max_rows {
            rows.push(std::mem::take(current));
        } else if let Some(last) = rows.last_mut() {
            if !current.is_empty() {
                last.push(' ');
                last.push_str(current);
                current.clear();
            }
        }
    }

    for (index, segment) in text.split('\n').enumerate() {
        if index > 0 {
            break_row(&mut rows, &mut current, max_rows);
        }

        for word in segment.split(' ') {
            if word.is_empty() {
                continue;
            }

            let word_width = font.text_width(word);

            if current.is_empty() {
                current.push_str(word);
            } else if rows.len() < max_rows
                && font.text_width(&current) + space + word_width > width
            {
                // `flag || (...)`: a word that does not fit starts the next row.
                rows.push(std::mem::take(&mut current));
                current.push_str(word);
            } else {
                current.push(' ');
                current.push_str(word);
            }
        }
    }

    if !current.is_empty() {
        break_row(&mut rows, &mut current, max_rows);
    }

    rows.retain(|row| !row.is_empty());
    rows.truncate(max_rows);

    rows
}

/// A row wider than the box: drawn into a row-sized buffer and composited back —
/// the same visible result as `Text_Blit`'s `swidth` clipping (`text.cpp:700-743`),
/// without writing outside the text box.
#[allow(clippy::too_many_arguments)]
fn draw_clipped_row(
    frame: &mut RgbaBuffer,
    x: i32,
    y: i32,
    text_area: PixelRect,
    font: &GnwFont,
    text: &str,
    color: Rgba,
    palette: &UiPalette,
) -> u32 {
    if text_area.width <= 0 || text_area.height <= 0 {
        return 0;
    }

    let height = (font.height() + 1).min(text_area.height.max(1));
    let mut boxed = RgbaBuffer::new(text_area.width as u32, height as u32);

    draw_text_gnw_shadowed(
        &mut boxed,
        0,
        0,
        font,
        text,
        color,
        palette.rgba(COLOR_BLACK),
        palette.rgba(COLOR_BLACK),
    );

    let mut painted = 0u32;
    let limit = (text_area.lry() + 1 - y).clamp(0, height);

    for dy in 0..limit {
        for dx in 0..boxed.width() as i32 {
            let Some(pixel) = boxed.get(dx, dy) else {
                continue;
            };

            if pixel.a == 0 {
                continue;
            }

            painted += u32::from(frame.set(x + dx, y + dy, pixel));
        }
    }

    painted
}

/// `DrawMap_UnitCompletionLabels` / `DrawMap_UnitCompletionLabels`
/// (`_(d7d0)`, `_(2ec0)`, `_(7c23)` — selected by `Unit::GetGender`, all equal in
/// en-US).
const NEW_UNIT_AVAILABLE: &str = "New {} available";

/// `_(e3a4)` — the finished research topic of a powered-on `RESEARCH`.
const RESEARCH_COMPLETED: &str = "{} research completed";

/// `DrawMap_RenderNamesDisplay` research branch (`drawmap.cpp:753-756`).
///
/// `unit->GetUnitType() == RESEARCH && unit->GetOrder() == ORDER_POWER_ON &&
/// UnitsManager_TeamInfo[team].research_topics[topic].turns_to_complete == 0`;
/// the topic label is `ResearchMenu_TopicLabels` ([`crate::researchmenu::TOPIC_LABELS`]).
fn research_completed_label(state: StateRef<'_>, unit: &UnitInfo) -> Option<String> {
    if unit.unit_type != UnitType::ResearchCentre || unit.orders != UnitOrder::PowerOn {
        return None;
    }

    let topic = usize::from(unit.research_topic as u8);
    let progress = state
        .teams()
        .get(unit.team.0 as usize)?
        .research_topics
        .get(topic)?;

    if progress.turns_to_complete != 0 {
        return None;
    }

    Some(RESEARCH_COMPLETED.replace("{}", crate::researchmenu::TOPIC_LABELS.get(topic)?))
}

/// Color of the completion labels — C++ palette index `0x1F`
/// (`GNW_TEXT_OUTLINE | 0x1F`).
///
/// The index has no fixed RGB: its **green** channel is the single-slot palette
/// cycle (`World::InitColorCycles`, `world.cpp:551`, 6 fps;
/// `World::UpdateColorAnimations`, `:222-231` rotates it `63 → 31 → 15 → 7 → 63`).
/// That cycle is exactly the original's blinking green label. The port draws RGBA,
/// so the index is resolved through the rotated palette snapshot
/// ([`App::animated_palette`]); before the first cycle step (`dt = 0`, QA/headless)
/// the slot keeps the cycle's initial value `63` (the `m_color_cycle_step`
/// constructor value) — the bright phase, so the goldens are deterministic and the
/// label is green.
pub fn ready_label_color<D: Dispatcher>(app: &App<D>) -> Rgba {
    if let Some(palette) = app.animated_palette() {
        return palette.rgba(COMPLETION_LABEL_COLOR);
    }

    let base = app
        .assets()
        .map(|assets| assets.system_palette.rgb(COMPLETION_LABEL_COLOR))
        .unwrap_or([0, 0, 0]);
    // `Color_SetSystemPaletteEntry` scales the 6-bit DAC value by 4.
    let green = max_render::palette::single_slot_value(0).saturating_mul(4);

    Rgba::from_rgb([base[0], green, base[2]])
}

/// Palette slot of the completion labels — `GNW_TEXT_OUTLINE | 0x1F`
/// (`drawmap.cpp:756`/`:769`).
pub const COMPLETION_LABEL_COLOR: u8 = 0x1F;

/// `DrawMap_RenderUnitDisplays` (`drawmap.cpp:543`) — COLORS/STATUS/HITS/AMMO
/// over the unit sprite, then `DrawMap_RenderNamesDisplay` (`:747`).
///
/// Called after [`crate::App::draw_units`], as in C++: `DrawMap_RenderUnit`
/// draws the sprite, and immediately after it — the indicators of this unit.
pub fn draw_unit_overlays<D: Dispatcher>(
    frame: &mut RgbaBuffer,
    app: &App<D>,
    viewport: &Viewport,
    palette: &UiPalette,
) -> u32 {
    let panel = &app.hud().panels;
    let state = app.state_ref();
    let player_team = state.player_team();
    let colors = panel.colors;
    let status = panel.status;
    let hits = panel.hits;
    let ammo = panel.ammo;
    let displays = colors || status || hits || ammo;

    let status_icons = app
        .assets()
        .and_then(|assets| assets.panels.as_ref())
        .map(|panels| &panels.status_icons);
    let zoom = viewport.zoom() as i32;
    let mut painted = 0u32;

    // No early return: `DrawMap_RenderNamesDisplay` (`drawmap.cpp:747`) also draws
    // the completion labels when every 3×3 panel button is off — they are not
    // part of `GameManager_DisplayButton*`.
    for id in state.unit_ids_in_order() {
        let Some(unit) = state.unit(id) else {
            continue;
        };

        // The debug `--no-fow` mode shows every unit and its indicators. The
        // visibility is the frame-side one (`App::presented_visible`): a foreign
        // unit appears when the own scan — at the sprite cells — reaches it.
        if !app.no_fow() && !app.presented_visible(unit) {
            continue;
        }

        // `UnitInfoGroup::IsRelevant` — idle units are not drawn, so they have
        // no indicators either.
        if unit.orders == UnitOrder::Idle
            || (unit.flags & unit_flags::GROUND_COVER != 0 && viewport.zoom() < 8)
        {
            continue;
        }

        // Death rule (`render::hull_sprite_drawn`): a destroyed unit has no sprite,
        // so its displays and labels go with it — the frame keeps only the death
        // animation on the death pixel.
        if !crate::render::hull_sprite_drawn(unit) {
            continue;
        }

        let visual = app
            .visual_position(id)
            .unwrap_or_else(|| crate::visual_target(unit));
        let (rect, size) = unit_screen_rect(unit, visual, viewport);

        // `DrawMap_RenderUnitDisplays` is inside the C++ `SELECTABLE` check;
        // `DrawMap_RenderNamesDisplay` (the labels) is not.
        if displays && unit.flags & unit_flags::SELECTABLE != 0 {
            if colors && unit.is_mobile() {
                painted += draw_color_frame(frame, rect, team_frame_color(palette, unit));
            }

            if status {
                if let Some(icons) = status_icons {
                    painted += draw_status_icons(frame, rect, unit, palette, icons);
                }
            }

            if zoom > 8 && (hits || ammo) {
                painted += draw_hit_ammo_bars(
                    frame,
                    rect,
                    size,
                    unit,
                    palette,
                    player_team,
                    zoom,
                    hits,
                    ammo,
                );
            }
        }

        painted += draw_unit_label(frame, app, viewport, palette, unit, rect, zoom);
    }

    painted
}

/// `GameManager_RenderScanRangeIndicators` (`game_manager.cpp:933-960`):
/// circles of radius `ATTRIB_RANGE` (RANGE) and `ATTRIB_SCAN` (SCAN) around
/// the selected unit.
///
/// The RANGE color is `COLOR_RED`, but `COLOR_CHROME_YELLOW` if the unit can hit
/// air (`Access_GetValidAttackTargetTypes(type) & MOBILE_AIR_UNIT`);
/// SCAN — `COLOR_YELLOW`.
pub fn draw_range_scan<D: Dispatcher>(
    frame: &mut RgbaBuffer,
    app: &App<D>,
    viewport: &Viewport,
    palette: &UiPalette,
) -> u32 {
    let panel = &app.hud().panels;

    if !panel.range && !panel.scan {
        return 0;
    }

    let state = app.state_ref();
    let player_team = state.player_team();
    let Some(id) = app.hud().selected else {
        return 0;
    };
    let Some(unit) = state.unit(id) else {
        return 0;
    };

    if !app.no_fow() && !unit.is_visible_to_team(player_team) {
        return 0;
    }

    // States in which C++ draws the indicators.
    let drawable = matches!(
        unit.state,
        UnitOrderState::ExecutingOrder
            | UnitOrderState::MoveGettingPath
            | UnitOrderState::ReadyToExecuteOrder
    ) || unit.orders == UnitOrder::Build;

    if !drawable {
        return 0;
    }

    let world_size = if unit.is_quad() {
        MAP_TILE_PIXELS
    } else {
        MAP_TILE_PIXELS / 2
    };
    // `bounds.ulx = grid_x * GFX_MAP_TILE_SIZE + unit_size` (world pixels).
    let center_world_x = i32::from(unit.grid.x) * MAP_TILE_PIXELS + world_size;
    let center_world_y = i32::from(unit.grid.y) * MAP_TILE_PIXELS + world_size;
    let (cx, cy) = viewport.world_to_screen(center_world_x, center_world_y);
    let zoom = viewport.zoom() as i32;
    let mut painted = 0u32;

    if panel.range && unit.orders != UnitOrder::Disable {
        let radius = unit
            .base_values
            .get_attribute(max_core::unit_values::ATTRIB_RANGE);

        if radius > 0 {
            let color = if max_core::access::valid_attack_target_types(unit.unit_type)
                & unit_flags::MOBILE_AIR_UNIT
                != 0
            {
                COLOR_CHROME_YELLOW
            } else {
                COLOR_RED
            };

            painted += draw_circle(frame, cx, cy, radius * zoom, palette.rgba(color));
        }
    }

    if panel.scan && unit.orders != UnitOrder::Disable {
        let radius = unit
            .base_values
            .get_attribute(max_core::unit_values::ATTRIB_SCAN);

        if radius > 0 {
            // `if (color == COLOR_YELLOW) ++scaled_radius;`
            painted += draw_circle(
                frame,
                cx,
                cy,
                radius * zoom + 1,
                palette.rgba(COLOR_YELLOW),
            );
        }
    }

    painted
}

/// `DrawMap_RenderMapTiles(..., display_button_grid)` (`drawmap.cpp:1050-1060`):
/// grid of cells over the tiles, under the units.
///
/// * `complex_grid_overlay = 0` → `DrawMap_RenderSimpleGridOverlay`: lines
///   in the **land** color (`GetGridOverlayColor(SURFACE_TYPE_LAND)`);
/// * `complex_grid_overlay = 1` → `DrawMap_RenderComplexGridOverlay`: the top and
///   left border of each cell in the color of its surface.
///
/// The screen positions come from [`max_render::viewport::Viewport::grid_lines`],
/// which closes the grid on **all four** map-window edges. The 7×7 map window is an
/// exact multiple of the 64-pixel cell, so its last boundary falls one pixel past
/// the map buffer; and because the camera origin is not cell-aligned in general,
/// the first boundary lies inside the window. Without the left/top closing lines
/// the border strip of the outer cells stayed outside the grid, and the panels
/// that abut the map made those cells look cut off (defect 7 of the manual run 1.2,
/// second pass).
pub fn draw_grid<D: Dispatcher>(
    frame: &mut RgbaBuffer,
    app: &App<D>,
    viewport: &Viewport,
    palette: &UiPalette,
    complex: bool,
) -> u32 {
    let state = app.state_ref();
    let width = frame.width() as i32;
    let height = frame.height() as i32;

    if width <= 0 || height <= 0 {
        return 0;
    }

    let lines = viewport.grid_lines(width, height);
    let mut painted = 0u32;

    if !complex {
        let color = grid_color(palette, Surface::LAND);

        for screen_x in lines.xs {
            for y in 0..height {
                if frame.set(screen_x, y, color) {
                    painted += 1;
                }
            }
        }

        for screen_y in lines.ys {
            for x in 0..width {
                if frame.set(x, screen_y, color) {
                    painted += 1;
                }
            }
        }

        return painted;
    }

    let (x0, y0) = viewport.screen_to_grid(0, 0);
    let (x1, y1) = viewport.screen_to_grid(width - 1, height - 1);
    let zoom = viewport.zoom() as i32;

    for grid_y in y0..=y1 {
        for grid_x in x0..=x1 {
            let surface = state.map().surface_at(grid_x, grid_y);
            let color = grid_color(palette, surface);
            let (screen_x, screen_y) = viewport.grid_to_screen(grid_x, grid_y);

            for offset in 0..=zoom {
                if frame.set(screen_x + offset, screen_y, color) {
                    painted += 1;
                }

                if frame.set(screen_x, screen_y + offset, color) {
                    painted += 1;
                }
            }
        }
    }

    // Closing left/top edges of the playable viewport. When the camera origin is
    // not cell-aligned the first cell boundary lies inside the map window, so the
    // outer strip of the border cells had no line at the window edge (defect 7).
    for y in 0..height {
        let (grid_x, grid_y) = viewport.screen_to_grid(0, y);
        let color = grid_color(palette, state.map().surface_at(grid_x, grid_y));

        if frame.set(0, y, color) {
            painted += 1;
        }
    }

    for x in 0..width {
        let (grid_x, grid_y) = viewport.screen_to_grid(x, 0);
        let color = grid_color(palette, state.map().surface_at(grid_x, grid_y));

        if frame.set(x, 0, color) {
            painted += 1;
        }
    }

    // Closing right/bottom edges of the playable viewport. The complex overlay
    // colors every border by the surface of the cell it belongs to, so the
    // closing line takes the color of the edge cell that touches it
    // (`world.cpp:536-539`), not the fixed land color of the simple grid.
    for y in 0..height {
        let (grid_x, grid_y) = viewport.screen_to_grid(width - 1, y);
        let color = grid_color(palette, state.map().surface_at(grid_x, grid_y));

        if frame.set(width - 1, y, color) {
            painted += 1;
        }
    }

    for x in 0..width {
        let (grid_x, grid_y) = viewport.screen_to_grid(x, height - 1);
        let color = grid_color(palette, state.map().surface_at(grid_x, grid_y));

        if frame.set(x, height - 1, color) {
            painted += 1;
        }
    }

    painted
}

#[cfg(test)]
#[path = "overlays_tests.rs"]
mod tests;
