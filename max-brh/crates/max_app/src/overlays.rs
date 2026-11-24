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
//! All flags are regular `CTInfo::display_button_*` toggles
//! (`GameManager_MenuClick*Button`, `game_manager.cpp:3800-3990`): a press
//! changes the flag and toggles the `*_ON` picture (`SetRestState`), and
//! `UpdateDrawBounds` redraws the map. In the port the flags live in
//! [`crate::panels::PanelState`], a button click already toggles them
//! ([`crate::ui`]) — here there is only drawing.

use max_core::enums::{unit_flags, Surface, UnitOrder, UnitOrderState};
use max_core::unit::UnitInfo;
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::text::{draw_text_gnw_shadowed, GnwFontId};
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

/// `DrawMap_RenderNamesDisplay` + `DrawMap_RenderTextBox` (`drawmap.cpp:692-777`).
///
/// Font by scale: `zoom_level = Gfx_ZoomLevel`, for non-buildings `/ 2`;
/// `>= 42` — `FONT_1`, `>= 24` — `FONT_5`, otherwise `FONT_2`. The C++ color —
/// `GNW_TEXT_OUTLINE | COLOR_YELLOW`; the port writes yellow with a black outline.
fn draw_unit_name(
    frame: &mut RgbaBuffer,
    unit: &UnitInfo,
    rect: PixelRect,
    zoom: i32,
    palette: &UiPalette,
) -> u32 {
    let building = unit.is_quad();
    // `text_area` = unit bounds, inset by 4 px (`drawmap.cpp:707-710`):
    // `ulx = (unit->x - unit_size/2) + 4`, `lrx - ulx = unit_size - 8`, where
    // `unit_size` 128/64 world px.
    let text_area = PixelRect::new(rect.ulx + 4, rect.uly + 4, (rect.width - 6).max(1), (rect.height - 6).max(1));
    let text = crate::panels::unit_name(unit);

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

    let width = font.text_width(&text);

    if width > text_area.width {
        return 0;
    }

    let x = text_area.ulx + (text_area.width - width) / 2;
    let y = text_area.uly + (text_area.height - font.height()) / 2;

    draw_text_gnw_shadowed(
        frame,
        x,
        y,
        font,
        &text,
        palette.rgba(COLOR_YELLOW),
        palette.rgba(COLOR_BLACK),
        palette.rgba(COLOR_BLACK),
    )
    .max(0) as u32
}

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
    let names = panel.names;

    if !(colors || status || hits || ammo || names) {
        return 0;
    }

    let status_icons = app
        .assets()
        .and_then(|assets| assets.panels.as_ref())
        .map(|panels| &panels.status_icons);
    let zoom = viewport.zoom() as i32;
    let mut painted = 0u32;

    for id in state.unit_ids_in_order() {
        let Some(unit) = state.unit(id) else {
            continue;
        };

        if !unit.is_visible_to_team(player_team) {
            continue;
        }

        // `UnitInfoGroup::IsRelevant` — idle units are not drawn, so they have
        // no indicators either.
        if unit.orders == UnitOrder::Idle
            || (unit.flags & unit_flags::GROUND_COVER != 0 && viewport.zoom() < 8)
        {
            continue;
        }

        if unit.flags & unit_flags::SELECTABLE == 0 {
            continue;
        }

        let visual = app
            .visual_position(id)
            .unwrap_or((f32::from(unit.grid.x), f32::from(unit.grid.y)));
        let (rect, size) = unit_screen_rect(unit, visual, viewport);

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

        if names && unit.team == player_team {
            painted += draw_unit_name(frame, unit, rect, zoom, palette);
        }
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

    if !unit.is_visible_to_team(player_team) {
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
/// which closes the grid at the last buffer row/column: the 7×7 map window is an
/// exact multiple of the 64-pixel cell, so its last boundary would fall one pixel
/// past the map buffer and the outer cells of the playable area stayed without a
/// border (defect 4 of the manual run 1.2 — a unit at the map edge visually slid
/// out of the grid, into the panels that overlap the border cells).
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
mod tests {
    use super::*;

    #[test]
    fn grid_colors_follow_the_world_overlay_table() {
        // `world.cpp:536-539`: RGB / 4 (integer), then `Color_MapColor`.
        assert_eq!(GRID_RGB_LAND, [12, 24, 2]);
        assert_eq!(GRID_RGB_WATER, [14, 20, 43]);
        assert_eq!(GRID_RGB_COAST, [20, 39, 5]);
        assert_eq!(GRID_RGB_AIR, [51, 8, 0]);
    }

    #[test]
    fn circle_is_drawn_on_the_border_only() {
        let mut frame = RgbaBuffer::filled(64, 64, Rgba::BLACK);
        let color = Rgba::from_rgb([255, 0, 0]);
        let painted = draw_circle(&mut frame, 32, 32, 10, color);

        assert!(painted > 0, "круг нарисован");
        assert_eq!(frame.get(32, 22), Some(color), "верхняя точка радиуса");
        assert_eq!(frame.get(32, 42), Some(color), "нижняя точка радиуса");
        assert_eq!(frame.get(22, 32), Some(color), "левая точка радиуса");
        assert_eq!(frame.get(42, 32), Some(color), "правая точка радиуса");
        assert_eq!(frame.get(32, 32), Some(Rgba::BLACK), "внутри пусто");
    }

    #[test]
    fn bar_display_fills_inclusive_rectangle() {
        let mut frame = RgbaBuffer::filled(64, 64, Rgba::BLACK);
        let color = Rgba::from_rgb([1, 2, 3]);
        let painted = fill_bar(&mut frame, 10, 10, 20, 12, color);

        assert_eq!(painted, 11 * 3);
        assert_eq!(frame.get(10, 10), Some(color));
        assert_eq!(frame.get(20, 12), Some(color));
        assert_eq!(frame.get(21, 10), Some(Rgba::BLACK));
    }
}
