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
