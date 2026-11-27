//! Drawing sprites into an RGBA buffer: palette, team colors, shadows.
//!
//! The indexing order repeats `Gfx_DecodeSprite` (`gfx.cpp:212`, §5.2 spw.md):
//! **team LUT first**, then the brightness LUT:
//! `color = palette[ brightness[ level ][ team_lut[index] ] ]`.
//!
//! The shadow is not pixels but a darkening mask (§4.3): [`SpriteRenderer::draw_shadow`]
//! applies the formula of the level 5 brightness table to already drawn pixels
//! (`c' = c * 5*32 / 224`). Transparent sprite pixels are not written at all, so
//! the "shadow -> sprite" order produces the original's picture
//! (`UnitInfo::RenderWithConnectors`).
//!
//! All of this is pure functions over [`RgbaBuffer`]: no window, no SDL, no state.

use max_assets::sprite::{ShadowFrame, SpriteFrame};

use crate::buffer::{PixelRect, Rgba, RgbaBuffer};
use crate::palette::{
    BrightnessLut, ColorLut, Palette, FULL_BRIGHTNESS_LEVEL, SHADOW_BRIGHTNESS_LEVEL,
};

/// Result of one drawing operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DrawStats {
    /// How many pixels were actually changed.
    pub drawn: u32,
    /// How many pixels did not fit into the buffer (clipping at the bounds).
    pub clipped: u32,
}

impl DrawStats {
    /// Sum of results (e.g. shadow + sprite).
    pub fn merge(self, other: Self) -> Self {
        Self {
            drawn: self.drawn + other.drawn,
            clipped: self.clipped + other.clipped,
        }
    }
}

/// Sprite renderer: palette + recolor tables + team.
///
/// The same renderer draws all frames of one unit: the team and the brightness
/// level are properties of the unit, not of the frame.
#[derive(Debug, Clone, Copy)]
pub struct SpriteRenderer<'a> {
    palette: &'a Palette,
    brightness: &'a BrightnessLut,
    team: ColorLut,
    level: u8,
}

impl<'a> SpriteRenderer<'a> {
    /// Renderer for team `team` and brightness level `level` (0..=7).
    pub fn new(
        palette: &'a Palette,
        brightness: &'a BrightnessLut,
        team: ColorLut,
        level: u8,
    ) -> Self {
        Self {
            palette,
            brightness,
            team,
            level: level.min(FULL_BRIGHTNESS_LEVEL),
        }
    }

    /// Renderer for neutral equipment (identity LUT) at full brightness.
    pub fn neutral(palette: &'a Palette, brightness: &'a BrightnessLut) -> Self {
        Self::new(
            palette,
            brightness,
            ColorLut::identity(),
            FULL_BRIGHTNESS_LEVEL,
        )
    }

    /// Palette color for a sprite index after the team LUT and the brightness LUT.
    #[inline]
    pub fn color_for(&self, index: u8) -> Rgba {
        let team_index = self.team.map(index);
        let level_index = self.brightness.map(self.level, team_index);

        self.palette.rgba(level_index)
    }

    /// Brightness level of the renderer.
    #[inline]
    pub fn level(&self) -> u8 {
        self.level
    }

    /// Team LUT of the renderer.
    #[inline]
    pub fn team(&self) -> &ColorLut {
        &self.team
    }

    /// Draws the frame so that the hot spot lands at `anchor`.
    ///
    /// Position — `anchor - hot` (§5.1 with `divisor = 2`, scale 1:1).
    /// Transparent pixels are not written. Outside the buffer — clipping.
    pub fn draw_frame(
        &self,
        dst: &mut RgbaBuffer,
        frame: &SpriteFrame,
        anchor: (i32, i32),
    ) -> DrawStats {
        let (ulx, uly) = self.frame_origin(frame, anchor);

        self.draw_rect(dst, frame, (ulx, uly), |_, index| self.color_for(index))
    }

    /// Draws the frame as a solid silhouette of the team color — the `zoom_level < 8` mode
    /// (`Gfx_TeamColorIndexBase != 0`, §5.2).
    pub fn draw_frame_silhouette(
        &self,
        dst: &mut RgbaBuffer,
        frame: &SpriteFrame,
        anchor: (i32, i32),
        color: Rgba,
    ) -> DrawStats {
        self.draw_frame_silhouette_scaled(dst, frame, anchor, 64, color)
    }

    /// Silhouette at scale `zoom` (64 — 1:1), nearest neighbour.
    ///
    /// The `zoom_level < 8` mode of `UnitInfo::RenderSprite` draws every opaque
    /// pixel of the sprite in the team color regardless of the palette, so the
    /// shadow/sprite geometry is the same as [`Self::draw_frame_scaled`] while
    /// the color is a constant (no palette lookup at all).
    pub fn draw_frame_silhouette_scaled(
        &self,
        dst: &mut RgbaBuffer,
        frame: &SpriteFrame,
        anchor: (i32, i32),
        zoom: u32,
        color: Rgba,
    ) -> DrawStats {
        let zoom = zoom.max(1);
        let (ulx, uly) = self.frame_origin_scaled(frame, anchor, zoom);
        let width = scale_length(frame.width, zoom);
        let height = scale_length(frame.height, zoom);
        let mut stats = DrawStats::default();

        for y in 0..height {
            let source_y = source_coordinate(y, zoom, i32::from(frame.height));

            for x in 0..width {
                let source_x = source_coordinate(x, zoom, i32::from(frame.width));

                if frame.pixel(source_x as u32, source_y as u32).is_none() {
                    continue;
                }

                if dst.set(ulx + x, uly + y, color) {
                    stats.drawn += 1;
                } else {
                    stats.clipped += 1;
                }
            }
        }

        stats
    }

    /// Darkens the already drawn map by the shadow mask.
    ///
    /// Transparent pixels of the shadow frame are skipped; the default darkening level
    /// is [`SHADOW_BRIGHTNESS_LEVEL`] (5). Only the already
    /// existing buffer pixels are darkened.
    pub fn draw_shadow(
        &self,
        dst: &mut RgbaBuffer,
        frame: &ShadowFrame,
        anchor: (i32, i32),
    ) -> DrawStats {
        self.draw_shadow_at_level(dst, frame, anchor, SHADOW_BRIGHTNESS_LEVEL)
    }

    /// The same, but with an explicit darkening level (0..=7).
    pub fn draw_shadow_at_level(
        &self,
        dst: &mut RgbaBuffer,
        frame: &ShadowFrame,
        anchor: (i32, i32),
        level: u8,
    ) -> DrawStats {
        let ulx = anchor.0 - i32::from(frame.hotx);
        let uly = anchor.1 - i32::from(frame.hoty);
        let mut stats = DrawStats::default();

        for y in 0..frame.height as i32 {
            for x in 0..frame.width as i32 {
                if !frame.darkened(x as u32, y as u32) {
                    continue;
                }

                if dst.modify(ulx + x, uly + y, |color| color.darken(level)) {
                    stats.drawn += 1;
                } else {
                    stats.clipped += 1;
                }
            }
        }

        stats
    }

    /// Draws a unit in the original's order: shadow (with the `shadow_offset` offset),
    /// then the sprite (`UnitInfo::RenderWithConnectors`, §5.4).
    pub fn draw_unit(
        &self,
        dst: &mut RgbaBuffer,
        frame: &SpriteFrame,
        shadow: Option<&ShadowFrame>,
        anchor: (i32, i32),
        shadow_offset: (i32, i32),
    ) -> DrawStats {
        let mut stats = DrawStats::default();

        if let Some(shadow) = shadow {
            stats = stats.merge(self.draw_shadow(
                dst,
                shadow,
                (anchor.0 + shadow_offset.0, anchor.1 + shadow_offset.1),
            ));
        }

        stats.merge(self.draw_frame(dst, frame, anchor))
    }

    /// Draws a unit with the camera scale (`zoom`, 64 — 1:1).
    ///
    /// Repeats `UnitInfo::RenderSprite`/`RenderShadow`: at `Gfx_ZoomLevel` the
    /// sprite and shadow are scaled together with the cell (`zoom / 64`), otherwise at a
    /// reduced/increased scale units look "glued" to the
    /// screen instead of the map. The hot spot and the shadow offset are scaled too.
    pub fn draw_unit_scaled(
        &self,
        dst: &mut RgbaBuffer,
        frame: &SpriteFrame,
        shadow: Option<&ShadowFrame>,
        anchor: (i32, i32),
        shadow_offset: (i32, i32),
        zoom: u32,
    ) -> DrawStats {
        let mut stats = DrawStats::default();
        let zoom = zoom.max(1);

        if let Some(shadow) = shadow {
            let scaled_offset = (
                scale_pixels(shadow_offset.0, zoom),
                scale_pixels(shadow_offset.1, zoom),
            );
            stats = stats.merge(self.draw_shadow_scaled(
                dst,
                shadow,
                (anchor.0 + scaled_offset.0, anchor.1 + scaled_offset.1),
                zoom,
            ));
        }

        stats.merge(self.draw_frame_scaled(dst, frame, anchor, zoom))
    }

    /// Draws the turret/gun on top of the hull — `UnitInfo::RenderWithConnectors`:
    /// the shadow and the sprite of the turret with the `offset` shift (`angle_offsets[angle]`) from
    /// the anchor of the hull.
    ///
    /// `offset` is given in pixels of the source sprite and is scaled together with
    /// `zoom` (64 — 1:1), as in C++ (`Gfx_MapScalingFactor`). The turret shadow
    /// is shifted by the same `shadow_offset` as the hull shadow.
    pub fn draw_turret_scaled(
        &self,
        dst: &mut RgbaBuffer,
        frame: &SpriteFrame,
        shadow: Option<&ShadowFrame>,
        anchor: (i32, i32),
        shadow_offset: (i32, i32),
        offset: (i32, i32),
        zoom: u32,
    ) -> DrawStats {
        let mut stats = DrawStats::default();
        let zoom = zoom.max(1);
        let scaled_offset = (scale_pixels(offset.0, zoom), scale_pixels(offset.1, zoom));
        let turret_anchor = (anchor.0 + scaled_offset.0, anchor.1 + scaled_offset.1);

        if let Some(shadow) = shadow {
            let scaled_shadow_offset = (
                scale_pixels(shadow_offset.0, zoom),
                scale_pixels(shadow_offset.1, zoom),
            );
            stats = stats.merge(self.draw_shadow_scaled(
                dst,
                shadow,
                (
                    turret_anchor.0 + scaled_shadow_offset.0,
                    turret_anchor.1 + scaled_shadow_offset.1,
                ),
                zoom,
            ));
        }

        stats.merge(self.draw_frame_scaled(dst, frame, turret_anchor, zoom))
    }

    /// Frame at scale `zoom` (64 — 1:1), nearest neighbor.
    pub fn draw_frame_scaled(
        &self,
        dst: &mut RgbaBuffer,
        frame: &SpriteFrame,
        anchor: (i32, i32),
        zoom: u32,
    ) -> DrawStats {
        let (ulx, uly) = self.frame_origin_scaled(frame, anchor, zoom);
        let width = scale_length(frame.width, zoom);
        let height = scale_length(frame.height, zoom);
        let mut stats = DrawStats::default();

        for y in 0..height {
            let source_y = source_coordinate(y, zoom, i32::from(frame.height));
            for x in 0..width {
                let source_x = source_coordinate(x, zoom, i32::from(frame.width));
                let Some(index) = frame.pixel(source_x as u32, source_y as u32) else {
                    continue;
                };

                if dst.set(ulx + x, uly + y, self.color_for(index)) {
                    stats.drawn += 1;
                } else {
                    stats.clipped += 1;
                }
            }
        }

        stats
    }

    /// Shadow at scale `zoom` (64 — 1:1), nearest neighbor.
    pub fn draw_shadow_scaled(
        &self,
        dst: &mut RgbaBuffer,
        frame: &ShadowFrame,
        anchor: (i32, i32),
        zoom: u32,
    ) -> DrawStats {
        let zoom = zoom.max(1);
        let ulx = anchor.0 - scale_pixels(i32::from(frame.hotx), zoom);
        let uly = anchor.1 - scale_pixels(i32::from(frame.hoty), zoom);
        let width = scale_length(frame.width, zoom);
        let height = scale_length(frame.height, zoom);
        let mut stats = DrawStats::default();

        for y in 0..height {
            let source_y = source_coordinate(y, zoom, i32::from(frame.height));
            for x in 0..width {
                let source_x = source_coordinate(x, zoom, i32::from(frame.width));
                if !frame.darkened(source_x as u32, source_y as u32) {
                    continue;
                }

                if dst.modify(ulx + x, uly + y, |color| color.darken(SHADOW_BRIGHTNESS_LEVEL)) {
                    stats.drawn += 1;
                } else {
                    stats.clipped += 1;
                }
            }
        }

        stats
    }

    /// `width × height` buffer with a single frame (anchor at the hot spot).
    ///
    /// Convenient for tests and for caching the frame as a texture.
    pub fn render_frame(&self, frame: &SpriteFrame) -> RgbaBuffer {
        let mut buffer = RgbaBuffer::new(u32::from(frame.width), u32::from(frame.height));

        self.draw_frame(
            &mut buffer,
            frame,
            (i32::from(frame.hotx), i32::from(frame.hoty)),
        );

        buffer
    }

    /// The rectangle the frame will occupy in buffer coordinates (before clipping).
    pub fn frame_rect(&self, frame: &SpriteFrame, anchor: (i32, i32)) -> PixelRect {
        let (ulx, uly) = self.frame_origin(frame, anchor);

        PixelRect::new(ulx, uly, i32::from(frame.width), i32::from(frame.height))
    }

    fn frame_origin(&self, frame: &SpriteFrame, anchor: (i32, i32)) -> (i32, i32) {
        (
            anchor.0 - i32::from(frame.hotx),
            anchor.1 - i32::from(frame.hoty),
        )
    }

    fn frame_origin_scaled(
        &self,
        frame: &SpriteFrame,
        anchor: (i32, i32),
        zoom: u32,
    ) -> (i32, i32) {
        (
            anchor.0 - scale_pixels(i32::from(frame.hotx), zoom),
            anchor.1 - scale_pixels(i32::from(frame.hoty), zoom),
        )
    }

    fn draw_rect(
        &self,
        dst: &mut RgbaBuffer,
        frame: &SpriteFrame,
        origin: (i32, i32),
        color: impl Fn(u32, u8) -> Rgba,
    ) -> DrawStats {
        let mut stats = DrawStats::default();
        let width = frame.width as i32;
        let height = frame.height as i32;

        for y in 0..height {
            for x in 0..width {
                let Some(index) = frame.pixel(x as u32, y as u32) else {
                    continue;
                };

                if dst.set(origin.0 + x, origin.1 + y, color(y as u32, index)) {
                    stats.drawn += 1;
                } else {
                    stats.clipped += 1;
                }
            }
        }

        stats
    }
}

/// Scale of a coordinate/hot spot: `value * zoom / 64` (64 — 1:1).
///
/// `pub(crate)` so that the HD sprite adapter (`crate::hd_sprites`) reuses the
/// exact geometry of the original renderer instead of re-deriving it.
pub(crate) fn scale_pixels(value: i32, zoom: u32) -> i32 {
    ((i64::from(value) * i64::from(zoom.max(1))) / 64) as i32
}

/// Scale of a length (minimum 1 pixel so that the sprite does not disappear).
pub(crate) fn scale_length(value: u16, zoom: u32) -> i32 {
    (((i64::from(value) * i64::from(zoom.max(1))) / 64) as i32).max(1)
}

/// Source coordinate for a destination pixel under nearest-neighbour.
pub(crate) fn source_coordinate(destination: i32, zoom: u32, source_len: i32) -> i32 {
    let source = (i64::from(destination) * 64 / i64::from(zoom.max(1))) as i32;
    source.min(source_len.saturating_sub(1)).max(0)
}

#[cfg(test)]
#[path = "sprite_atlas_tests.rs"]
mod tests;
