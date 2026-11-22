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
        let (ulx, uly) = self.frame_origin(frame, anchor);

        self.draw_rect(dst, frame, (ulx, uly), |_, _| color)
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
fn scale_pixels(value: i32, zoom: u32) -> i32 {
    ((i64::from(value) * i64::from(zoom.max(1))) / 64) as i32
}

/// Scale of a length (minimum 1 pixel so that the sprite does not disappear).
fn scale_length(value: u16, zoom: u32) -> i32 {
    (((i64::from(value) * i64::from(zoom.max(1))) / 64) as i32).max(1)
}

/// Source coordinate for a destination pixel under nearest-neighbour.
fn source_coordinate(destination: i32, zoom: u32, source_len: i32) -> i32 {
    let source = (i64::from(destination) * 64 / i64::from(zoom.max(1))) as i32;
    source.min(source_len.saturating_sub(1)).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{Palette, PALETTE_BYTES};
    use max_assets::sprite::{decode_shadow, decode_sprite};

    /// Palette whose 8-bit color is `((index % 64) * 4, ...)`.
    fn ramp_palette() -> Palette {
        let mut bytes = vec![0u8; PALETTE_BYTES];

        for index in 0..256usize {
            let value = (index % 64) as u8;

            bytes[index * 3] = value;
            bytes[index * 3 + 1] = value;
            bytes[index * 3 + 2] = value;
        }

        Palette::from_color_pal(&bytes).unwrap()
    }

    /// Assembles a body from one frame; `rows` are ready encoded rows.
    fn build_sprite(width: u16, height: u16, hotx: i16, hoty: i16, rows: Vec<Vec<u8>>) -> Vec<u8> {
        assert_eq!(rows.len(), height as usize);

        let mut out = Vec::new();

        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&6u32.to_le_bytes());
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&hotx.to_le_bytes());
        out.extend_from_slice(&hoty.to_le_bytes());

        let table = out.len();
        out.extend(std::iter::repeat_n(0u8, 4 * height as usize));

        for (y, row) in rows.iter().enumerate() {
            let offset = out.len() as u32;
            out[table + 4 * y..table + 4 * y + 4].copy_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(row);
        }

        out
    }

    /// 3×2 sprite: row 0 — two pixels `1, 2` with skip 1; row 1 — pixel `3`.
    fn sample_sprite() -> Vec<u8> {
        build_sprite(
            3,
            2,
            1,
            1,
            vec![vec![1, 2, 1, 2, 0xFF], vec![0, 1, 3, 0xFF]],
        )
    }

    fn renderer<'a>(palette: &'a Palette, brightness: &'a BrightnessLut) -> SpriteRenderer<'a> {
        SpriteRenderer::neutral(palette, brightness)
    }

    #[test]
    fn frame_pixels_are_palette_colors_and_anchor_uses_hot_spot() {
        let palette = ramp_palette();
        let brightness = BrightnessLut::build(&palette);
        let sheet = decode_sprite(&sample_sprite()).unwrap();
        let frame = sheet.frame(0).unwrap();
        let renderer = renderer(&palette, &brightness);

        let buffer = renderer.render_frame(frame);

        assert_eq!(buffer.width(), 3);
        assert_eq!(buffer.height(), 2);
        // Row 0: (0,0) is transparent, (1,0) = 1, (2,0) = 2.
        assert_eq!(buffer.get(0, 0), Some(Rgba::TRANSPARENT));
        assert_eq!(buffer.get(1, 0), Some(palette.rgba(1)));
        assert_eq!(buffer.get(2, 0), Some(palette.rgba(2)));
        // Row 1: (0,1) = 3, the rest is transparent.
        assert_eq!(buffer.get(0, 1), Some(palette.rgba(3)));
        assert_eq!(buffer.get(1, 1), Some(Rgba::TRANSPARENT));

        // In an 8×8 buffer, anchor (4,4) with hot (1,1) -> top-left corner (3,3).
        let mut target = RgbaBuffer::new(8, 8);
        let stats = renderer.draw_frame(&mut target, frame, (4, 4));

        assert_eq!(
            stats,
            DrawStats {
                drawn: 3,
                clipped: 0
            }
        );
        assert_eq!(target.get(3, 3), Some(Rgba::TRANSPARENT));
        assert_eq!(target.get(4, 3), Some(palette.rgba(1)));
        assert_eq!(target.get(5, 3), Some(palette.rgba(2)));
        assert_eq!(target.get(3, 4), Some(palette.rgba(3)));
        assert_eq!(
            renderer.frame_rect(frame, (4, 4)),
            PixelRect::new(3, 3, 3, 2)
        );
    }

    #[test]
    fn command_slots_are_recoloured_by_team_lut() {
        let palette = ramp_palette();
        let brightness = BrightnessLut::build(&palette);
        // Index 34 is team slot 32–39.
        let body = build_sprite(2, 1, 0, 0, vec![vec![0, 2, 34, 35, 0xFF]]);
        let sheet = decode_sprite(&body).unwrap();
        let frame = sheet.frame(0).unwrap();

        let red = SpriteRenderer::new(
            &palette,
            &brightness,
            ColorLut::for_team(crate::palette::TeamColors::Red),
            FULL_BRIGHTNESS_LEVEL,
        );
        let green = SpriteRenderer::new(
            &palette,
            &brightness,
            ColorLut::for_team(crate::palette::TeamColors::Green),
            FULL_BRIGHTNESS_LEVEL,
        );

        // Red team: 34 -> 58, 35 -> 59.
        assert_eq!(red.color_for(34), palette.rgba(58));
        assert_eq!(red.color_for(35), palette.rgba(59));
        // Green: slots 32–39 are native indices, no recolor.
        assert_eq!(green.color_for(34), palette.rgba(34));
        assert_eq!(green.color_for(35), palette.rgba(35));

        let red_buffer = red.render_frame(frame);
        let green_buffer = green.render_frame(frame);

        // Red: 34 -> 58, 35 -> 59; green — no recolor.
        assert_eq!(red_buffer.get(0, 0), Some(palette.rgba(58)));
        assert_eq!(red_buffer.get(1, 0), Some(palette.rgba(59)));
        assert_eq!(green_buffer.get(0, 0), Some(palette.rgba(34)));
        assert_eq!(green_buffer.get(1, 0), Some(palette.rgba(35)));
        assert_ne!(red_buffer.get(1, 0), green_buffer.get(1, 0));
    }

    #[test]
    fn brightness_level_changes_all_opaque_pixels() {
        let palette = ramp_palette();
        let brightness = BrightnessLut::build(&palette);
        let sheet = decode_sprite(&sample_sprite()).unwrap();
        let frame = sheet.frame(0).unwrap();

        let full = SpriteRenderer::neutral(&palette, &brightness);
        let dim = SpriteRenderer::new(
            &palette,
            &brightness,
            ColorLut::identity(),
            SHADOW_BRIGHTNESS_LEVEL,
        );

        let bright_buffer = full.render_frame(frame);
        let dim_buffer = dim.render_frame(frame);

        // Index 1 at level 5 goes to a darker palette color.
        assert_eq!(bright_buffer.get(1, 0), Some(palette.rgba(1)));
        assert_eq!(
            dim_buffer.get(1, 0),
            Some(palette.rgba(brightness.map(5, 1)))
        );
        assert_ne!(bright_buffer.get(1, 0), dim_buffer.get(1, 0));
        // Full brightness is the identity.
        assert_eq!(brightness.map(FULL_BRIGHTNESS_LEVEL, 1), 1);
    }

    #[test]
    fn scaled_sprite_follows_zoom_and_hot_spot() {
        let palette = ramp_palette();
        let brightness = BrightnessLut::build(&palette);
        let renderer = renderer(&palette, &brightness);
        let body = build_sprite(
            2,
            2,
            1,
            1,
            vec![vec![0, 2, 1, 2, 0xFF], vec![0, 2, 1, 2, 0xFF]],
        );
        let sheet = decode_sprite(&body).unwrap();
        let frame = sheet.frame(0).unwrap();

        // 1:1 — 2×2, hot spot (1,1) at the anchor.
        let mut native = RgbaBuffer::filled(16, 16, Rgba::TRANSPARENT);
        renderer.draw_frame_scaled(&mut native, frame, (8, 8), 64);
        assert_eq!(native.get(7, 7), Some(palette.rgba(1)));
        assert_eq!(native.get(8, 7), Some(palette.rgba(2)));
        assert_eq!(native.get(7, 8), Some(palette.rgba(1)));
        assert_eq!(native.get(8, 8), Some(palette.rgba(2)));

        // 2× — every pixel of the 2×2 block, the hot spot is ×2 as well.
        let mut zoomed = RgbaBuffer::filled(24, 24, Rgba::TRANSPARENT);
        renderer.draw_frame_scaled(&mut zoomed, frame, (12, 12), 128);
        for y in 10..14 {
            for x in 10..14 {
                let expected = if x < 12 { 1 } else { 2 };
                assert_eq!(zoomed.get(x, y), Some(palette.rgba(expected)));
            }
        }
        assert_eq!(zoomed.get(10, 10), Some(palette.rgba(1)));
        assert_eq!(zoomed.get(13, 13), Some(palette.rgba(2)));

        // 1/2 — one pixel, the top-left source pixel lands in it.
        let mut half = RgbaBuffer::filled(16, 16, Rgba::TRANSPARENT);
        renderer.draw_frame_scaled(&mut half, frame, (8, 8), 32);
        assert_eq!(half.get(8, 8), Some(palette.rgba(1)));
        assert_eq!(half.get(7, 8), Some(Rgba::TRANSPARENT));
    }

    #[test]
    fn shadow_darkens_existing_pixels_only() {
        let palette = ramp_palette();
        let brightness = BrightnessLut::build(&palette);
        let renderer = renderer(&palette, &brightness);

        // Shadow 4×1: {1, 2} -> x = 1, 2 are darkened.
        let body = build_sprite(4, 1, 0, 0, vec![vec![1, 2, 0xFF]]);
        let shadow = decode_shadow(&body).unwrap();
        let frame = shadow.frame(0).unwrap();

        let base = Rgba::rgb(210, 140, 70);
        let mut target = RgbaBuffer::filled(4, 2, base);
        let stats = renderer.draw_shadow(&mut target, frame, (0, 0));

        assert_eq!(
            stats,
            DrawStats {
                drawn: 2,
                clipped: 0
            }
        );
        // Level 5: 210 -> 150, 140 -> 100, 70 -> 50.
        assert_eq!(target.get(0, 0), Some(base));
        assert_eq!(target.get(1, 0), Some(base.darken(5)));
        assert_eq!(target.get(2, 0), Some(base.darken(5)));
        assert_eq!(target.get(3, 0), Some(base));
        // The second row is unaffected: the shadow has height = 1.
        assert_eq!(target.get(1, 1), Some(base));
        assert_eq!(base.darken(5), Rgba::rgb(150, 100, 50));
    }

    #[test]
    fn draw_unit_paints_shadow_before_sprite() {
        let palette = ramp_palette();
        let brightness = BrightnessLut::build(&palette);
        let renderer = renderer(&palette, &brightness);

        let sprite = decode_sprite(&build_sprite(2, 1, 0, 0, vec![vec![0, 1, 9, 0xFF]])).unwrap();
        let shadow = decode_shadow(&build_sprite(2, 1, 0, 0, vec![vec![0, 2, 0xFF]])).unwrap();

        let mut target = RgbaBuffer::filled(2, 1, Rgba::rgb(255, 255, 255));
        let stats = renderer.draw_unit(
            &mut target,
            sprite.frame(0).unwrap(),
            Some(shadow.frame(0).unwrap()),
            (0, 0),
            (0, 0),
        );

        assert_eq!(stats.drawn, 3);
        // Pixel 1: first darkened (shadow), then covered by the sprite (index 9).
        assert_eq!(target.get(0, 0), Some(palette.rgba(9)));
        assert_eq!(target.get(1, 0), Some(Rgba::rgb(255, 255, 255).darken(5)));
    }

    #[test]
    fn clipping_counts_pixels_outside_the_buffer() {
        let palette = ramp_palette();
        let brightness = BrightnessLut::build(&palette);
        let sheet = decode_sprite(&sample_sprite()).unwrap();
        let frame = sheet.frame(0).unwrap();
        let renderer = renderer(&palette, &brightness);

        let mut target = RgbaBuffer::new(2, 1);
        // Anchor (1,1) with hot (1,1) -> the frame starts at (0,0), but the height
        // of the buffer is only 1 pixel: pixels (2,0) and (0,1) of the frame are cut off.
        let stats = renderer.draw_frame(&mut target, frame, (1, 1));

        assert_eq!(
            stats,
            DrawStats {
                drawn: 1,
                clipped: 2
            }
        );
        assert_eq!(target.get(1, 0), Some(palette.rgba(1)));
        assert_eq!(target.get(0, 0), Some(Rgba::TRANSPARENT));
    }

    #[test]
    fn silhouette_mode_ignores_palette() {
        let palette = ramp_palette();
        let brightness = BrightnessLut::build(&palette);
        let sheet = decode_sprite(&sample_sprite()).unwrap();
        let frame = sheet.frame(0).unwrap();
        let renderer = renderer(&palette, &brightness);

        let color = Rgba::rgb(200, 30, 40);
        let mut target = RgbaBuffer::new(8, 8);
        let stats = renderer.draw_frame_silhouette(&mut target, frame, (4, 4), color);

        assert_eq!(stats.drawn, 3);
        assert_eq!(target.get(4, 3), Some(color));
        assert_eq!(target.get(5, 3), Some(color));
        assert_eq!(target.get(3, 4), Some(color));
        assert_eq!(target.get(3, 3), Some(Rgba::TRANSPARENT));
    }
}
