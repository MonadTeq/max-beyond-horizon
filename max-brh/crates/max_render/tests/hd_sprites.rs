//! Integration tests of the R4 HD unit sprites through the public API
//! (`max-brh-docs/development/test-organization.md`: unit → `src/`,
//! integration → `tests/`).
//!
//! The sprite bodies come from the real `TANK`/`S_TANK` fixtures of
//! `max_assets/tests/data`; the HD index maps are synthetically built as exact
//! `×S` nearest upscales of those frames, which is the shape of an R4 asset
//! before the R2 encoder.
//!
//! The **R4 gate** checked here:
//!
//! 1. for every team (`ColorLut`), every brightness level `0..=7`
//!    (`BrightnessLut`) and every palette-cycle frame (`ColorCycles`), the SD
//!    sprite at `×S` is exactly the `×S` upscale of the 1:1
//!    [`SpriteRenderer`] path — the recolor semantics are the original ones;
//! 2. `S = 1` is byte for byte the original renderer at every zoom;
//! 3. the HD frame occupies exactly the rectangle the original frame occupies
//!    at the HD zoom, and samples it with the documented nearest mapping;
//! 4. a frame without an HD asset (and the whole bank without a manifest) is
//!    the `×S` upscale of the original path — the per-asset fallback;
//! 5. HD shadows darken exactly like `Rgba::darken(5)`;
//! 6. the `zoom < 8` silhouette paints the coverage in the team color and
//!    ignores the palette.

use std::path::{Path, PathBuf};

use max_assets::remaster::{sprite_frame_tag, sprite_shadow_tag, RemasterCache, RemasterManifest};
use max_assets::sprite::{
    decode_shadow, decode_sprite, ShadowFrame, ShadowSheet, SpriteFrame, SpriteSheet,
};
use max_render::buffer::{Rgba, RgbaBuffer};
use max_render::hd_sprites::{
    is_silhouette_zoom, HdIndexFrame, HdIndexMap, HdRecolorState, HdShadowFrame, HdSpriteBank,
    HdSpriteCache, HdSpriteRenderer, HdSpriteTexture, IndexPaletteLut, SILHOUETTE_ZOOM,
};
use max_render::palette::{
    rotate_palette, BrightnessLut, ColorCycles, Palette, TeamColors, FULL_BRIGHTNESS_LEVEL,
    PALETTE_BYTES, PALETTE_SIZE, SHADOW_BRIGHTNESS_LEVEL,
};
use max_render::render_scale::RenderScale;
use max_render::sprite_atlas::SpriteRenderer;
use max_render::upscale_nearest;

/// Zooms the original supports (`Gfx_ZoomLevel` 8..128).
const ZOOMS: [u32; 6] = [8, 16, 32, 64, 96, 128];
/// `1:1` zoom of the original frame (`Gfx_ZoomLevel` 64).
const NATIVE_ZOOM: u32 = 64;
/// Logical canvas and anchor: big enough for the `TANK` frame plus a shifted
/// shadow, and independent of the presentation scale.
const CANVAS: i32 = 96;
const ANCHOR: (i32, i32) = (48, 48);
/// A non-zero shadow shift, in source sprite pixels (the original moves the
/// shadow by `shadow_offset` and scales it with the map zoom).
const SHADOW_OFFSET: (i32, i32) = (-12, -8);

fn assets_data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("max_assets")
        .join("tests")
        .join("data")
        .join(name)
}

fn remaster_root() -> PathBuf {
    assets_data("remaster")
}

/// Palette whose 8-bit color is `((index % 64) * 4, ...)` — the ramp of the
/// `sprite_atlas` unit tests.
fn ramp_palette() -> Palette {
    let mut bytes = vec![0u8; PALETTE_BYTES];

    for index in 0..PALETTE_SIZE {
        let value = (index % 64) as u8;

        bytes[index * 3] = value;
        bytes[index * 3 + 1] = value;
        bytes[index * 3 + 2] = value;
    }

    Palette::from_color_pal(&bytes).unwrap()
}

fn tank() -> (SpriteSheet, ShadowSheet) {
    let sprite = decode_sprite(&std::fs::read(assets_data("sprite_TANK.bin")).unwrap())
        .expect("TANK sprite fixture");
    let shadow = decode_shadow(&std::fs::read(assets_data("sprite_S_TANK.bin")).unwrap())
        .expect("S_TANK shadow fixture");

    (sprite, shadow)
}

/// The first frame that has a shadow (the shadow sheet may be shorter).
fn frame_with_shadow(sheet: &SpriteSheet, shadows: &ShadowSheet) -> usize {
    (0..sheet.len())
        .find(|index| shadows.frame(*index).is_some())
        .expect("the TANK fixture must have a shadow for at least one frame")
}

/// The exact `×factor` nearest upscale of a body frame into an HD index map.
fn upscaled_frame(frame: &SpriteFrame, factor: u32) -> HdIndexFrame {
    let width = u32::from(frame.width) * factor;
    let height = u32::from(frame.height) * factor;
    let mut payload = vec![0u8; (width * height) as usize];
    let mut coverage = vec![0u8; (width * height) as usize];

    for y in 0..height {
        for x in 0..width {
            let Some(index) = frame.pixel(x / factor, y / factor) else {
                continue;
            };

            let texel = (y * width + x) as usize;
            payload[texel] = index;
            coverage[texel] = u8::MAX;
        }
    }

    HdIndexFrame::new(
        HdIndexMap::from_indices(width, height, payload, coverage).expect("valid HD frame"),
        scaled_hot(frame.hotx, frame.width, width),
        scaled_hot(frame.hoty, frame.height, height),
    )
}

/// The exact `×factor` nearest upscale of a shadow mask: `255` where the
/// original darkens, `0` elsewhere (a hard-edged mask is exactly `darken(5)`).
fn upscaled_shadow(frame: &ShadowFrame, factor: u32) -> HdShadowFrame {
    let width = u32::from(frame.width) * factor;
    let height = u32::from(frame.height) * factor;
    let mut mask = vec![0u8; (width * height) as usize];

    for y in 0..height {
        for x in 0..width {
            if frame.darkened(x / factor, y / factor) {
                mask[(y * width + x) as usize] = u8::MAX;
            }
        }
    }

    HdShadowFrame::new(
        HdIndexMap::from_indices(
            width,
            height,
            mask,
            vec![u8::MAX; (width * height) as usize],
        )
        .expect("valid HD shadow"),
        scaled_hot(frame.hotx, frame.width, width),
        scaled_hot(frame.hoty, frame.height, height),
    )
}

fn scaled_hot(hot: i16, source_len: u16, target_len: u32) -> i16 {
    let scaled = i64::from(hot) * i64::from(target_len) / i64::from(source_len);

    scaled.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16
}

/// A bank with the HD frame/shadow of `index` and nothing else.
fn bank_with(
    sheet: &SpriteSheet,
    shadows: &ShadowSheet,
    scale: RenderScale,
    index: usize,
    frame: Option<HdIndexFrame>,
    shadow: Option<HdShadowFrame>,
) -> HdSpriteBank {
    let mut frames = vec![None; sheet.len()];
    let mut hd_shadows = vec![None; sheet.len()];
    frames[index] = frame;
    hd_shadows[index] = shadow;

    HdSpriteBank::with_frames(
        sheet.clone(),
        Some(shadows.clone()),
        scale,
        frames,
        hd_shadows,
    )
}

/// A logical `CANVAS × CANVAS` buffer with `ANCHOR`; the HD counterpart is the
/// same picture at scale `S` (`CANVAS·S` texels, anchor `ANCHOR·S`).
fn canvas(scale: RenderScale) -> (RgbaBuffer, (i32, i32)) {
    let side = CANVAS as u32 * scale.scale();
    let anchor = (
        ANCHOR.0 * scale.scale() as i32,
        ANCHOR.1 * scale.scale() as i32,
    );

    (RgbaBuffer::filled(side, side, Rgba::TRANSPARENT), anchor)
}

/// Palette-cycle frames the gate sweeps: the initial one and two advanced ones
/// (beacon/drill ranges: 9/6/2/8/10 fps).
fn cycle_frames() -> [ColorCycles; 3] {
    let mut one_second = ColorCycles::new();
    one_second.advance(1.0);

    let mut two_and_a_half = ColorCycles::new();
    two_and_a_half.advance(2.5);

    [ColorCycles::new(), one_second, two_and_a_half]
}

// ---------------------------------------------------------------------------
// 1. R4 gate: team + brightness 0..=7 + cycles equal the 1:1 SpriteRenderer
// ---------------------------------------------------------------------------

#[test]
fn hd_sprites_match_the_sprite_renderer_for_every_team_brightness_and_cycle() {
    let (sheet, shadows) = tank();
    let index = frame_with_shadow(&sheet, &shadows);
    let frame = sheet.frame(index).unwrap();
    let shadow = shadows.frame(index).unwrap();
    let palette = ramp_palette();
    let brightness = BrightnessLut::build(&palette);

    for render_scale in [RenderScale::Hd2, RenderScale::Hd4] {
        let factor = render_scale.scale();
        let bank = bank_with(
            &sheet,
            &shadows,
            render_scale,
            index,
            Some(upscaled_frame(frame, factor)),
            Some(upscaled_shadow(shadow, factor)),
        );

        for team in TeamColors::ALL {
            for level in 0..=FULL_BRIGHTNESS_LEVEL {
                for cycles in cycle_frames() {
                    let state = HdRecolorState::new(team, level).with_cycles(&cycles);
                    let hd = HdSpriteRenderer::new(&bank, &palette, &brightness, state);
                    let mut cache = HdSpriteCache::new();
                    let (mut hd_frame, hd_anchor) = canvas(render_scale);

                    let hd_stats = hd.draw_unit_scaled(
                        &mut hd_frame,
                        &mut cache,
                        index,
                        hd_anchor,
                        SHADOW_OFFSET,
                        NATIVE_ZOOM * factor,
                    );

                    // The reference: the original renderer at 1:1 with the same
                    // (cycle-rotated) palette, upscaled ×S afterwards.
                    let snapshot = rotate_palette(&palette, &cycles);
                    let renderer = SpriteRenderer::new(&snapshot, &brightness, team.lut(), level);
                    let (mut logical, logical_anchor) = canvas(RenderScale::PixelPerfect);
                    let reference_stats = renderer.draw_unit_scaled(
                        &mut logical,
                        frame,
                        Some(shadow),
                        logical_anchor,
                        SHADOW_OFFSET,
                        NATIVE_ZOOM,
                    );
                    let reference =
                        upscale_nearest(&logical, CANVAS as u32 * factor, CANVAS as u32 * factor);

                    assert_eq!(
                        hd_stats.drawn,
                        reference_stats.drawn * factor * factor,
                        "every drawn pixel of the 1:1 frame becomes an ×{factor} block \
                         (team {team:?}, level {level}, {render_scale:?}, cycles {:?})",
                        cycles.step_signature()
                    );
                    assert_eq!(
                        hd_stats.clipped,
                        reference_stats.clipped * factor * factor,
                        "clipping scales the same way (team {team:?}, level {level})"
                    );
                    assert_eq!(
                        hd_frame,
                        reference,
                        "HD ×{factor} must be the ×{factor} upscale of the 1:1 SpriteRenderer \
                         path (team {team:?}, level {level}, cycles {:?})",
                        cycles.step_signature()
                    );
                }
            }
        }
    }
}

#[test]
fn team_colors_actually_differ_in_the_hd_path() {
    let (sheet, shadows) = tank();
    let index = frame_with_shadow(&sheet, &shadows);
    let palette = ramp_palette();
    let brightness = BrightnessLut::build(&palette);
    let bank = bank_with(
        &sheet,
        &shadows,
        RenderScale::Hd2,
        index,
        Some(upscaled_frame(sheet.frame(index).unwrap(), 2)),
        None,
    );

    let mut rendered = Vec::new();

    for team in TeamColors::ALL {
        let hd = HdSpriteRenderer::new(
            &bank,
            &palette,
            &brightness,
            HdRecolorState::new(team, FULL_BRIGHTNESS_LEVEL),
        );
        let mut cache = HdSpriteCache::new();
        let (mut target, anchor) = canvas(RenderScale::Hd2);
        hd.draw_frame_scaled(&mut target, &mut cache, index, anchor, NATIVE_ZOOM * 2);
        rendered.push(target.pixels().to_vec());
    }

    for (first, first_pixels) in rendered.iter().enumerate() {
        for (second, second_pixels) in rendered.iter().enumerate().skip(first + 1) {
            assert_ne!(
                first_pixels, second_pixels,
                "team sets {first} and {second} must recolor the command slots differently"
            );
        }
    }
}

#[test]
fn brightness_levels_darken_the_hd_path_monotonically() {
    let (sheet, shadows) = tank();
    let index = frame_with_shadow(&sheet, &shadows);
    let palette = ramp_palette();
    let brightness = BrightnessLut::build(&palette);
    let bank = bank_with(
        &sheet,
        &shadows,
        RenderScale::Hd2,
        index,
        Some(upscaled_frame(sheet.frame(index).unwrap(), 2)),
        None,
    );

    let luminance = |level: u8| {
        let hd = HdSpriteRenderer::new(
            &bank,
            &palette,
            &brightness,
            HdRecolorState::new(TeamColors::Green, level),
        );
        let mut cache = HdSpriteCache::new();
        let (mut target, anchor) = canvas(RenderScale::Hd2);
        hd.draw_frame_scaled(&mut target, &mut cache, index, anchor, NATIVE_ZOOM * 2);

        target
            .pixels()
            .chunks_exact(4)
            .map(|pixel| {
                // Brightness is a per-index table, so the ordering is only
                // meaningful on the palette ramp: compare the LUT colors.
                u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2])
            })
            .sum::<u32>()
    };

    let full = luminance(FULL_BRIGHTNESS_LEVEL);
    let mid = luminance(3);
    let dark = luminance(0);

    assert_eq!(luminance(FULL_BRIGHTNESS_LEVEL), full, "deterministic");
    assert!(
        dark <= mid && mid <= full,
        "level 0 ≤ level 3 ≤ level 7 ({dark} ≤ {mid} ≤ {full})"
    );
    assert!(
        dark < full,
        "the darkest level must differ from the full one"
    );
}

// ---------------------------------------------------------------------------
// 2. S = 1 is byte for byte the original renderer
// ---------------------------------------------------------------------------

#[test]
fn pixel_perfect_hd_is_byte_identical_to_the_original_path() {
    let (sheet, shadows) = tank();
    let index = frame_with_shadow(&sheet, &shadows);
    let frame = sheet.frame(index).unwrap();
    let shadow = shadows.frame(index).unwrap();
    let palette = ramp_palette();
    let brightness = BrightnessLut::build(&palette);
    let bank = bank_with(
        &sheet,
        &shadows,
        RenderScale::PixelPerfect,
        index,
        Some(upscaled_frame(frame, 1)),
        Some(upscaled_shadow(shadow, 1)),
    );

    for team in TeamColors::ALL {
        for level in [0u8, 3, FULL_BRIGHTNESS_LEVEL] {
            let renderer = SpriteRenderer::new(&palette, &brightness, team.lut(), level);
            let hd = HdSpriteRenderer::new(
                &bank,
                &palette,
                &brightness,
                HdRecolorState::new(team, level),
            );

            for zoom in ZOOMS {
                let mut cache = HdSpriteCache::new();
                let (mut hd_frame, anchor) = canvas(RenderScale::PixelPerfect);
                let (mut original, original_anchor) = canvas(RenderScale::PixelPerfect);

                let hd_stats = hd.draw_unit_scaled(
                    &mut hd_frame,
                    &mut cache,
                    index,
                    anchor,
                    SHADOW_OFFSET,
                    zoom,
                );
                let original_stats = renderer.draw_unit_scaled(
                    &mut original,
                    frame,
                    Some(shadow),
                    original_anchor,
                    SHADOW_OFFSET,
                    zoom,
                );

                assert_eq!(anchor, original_anchor);
                assert_eq!(
                    hd_stats, original_stats,
                    "stats (team {team:?}, level {level}, zoom {zoom})"
                );
                assert_eq!(
                    hd_frame, original,
                    "S = 1 must stay byte for byte the SpriteRenderer output \
                     (team {team:?}, level {level}, zoom {zoom})"
                );
            }
        }
    }
}

#[test]
fn pixel_perfect_silhouette_is_byte_identical_to_the_original_path() {
    let (sheet, shadows) = tank();
    let index = frame_with_shadow(&sheet, &shadows);
    let frame = sheet.frame(index).unwrap();
    let palette = ramp_palette();
    let brightness = BrightnessLut::build(&palette);
    let bank = bank_with(
        &sheet,
        &shadows,
        RenderScale::PixelPerfect,
        index,
        Some(upscaled_frame(frame, 1)),
        None,
    );
    let hd = HdSpriteRenderer::new(&bank, &palette, &brightness, HdRecolorState::default());
    let renderer = SpriteRenderer::neutral(&palette, &brightness);
    let color = Rgba::rgb(200, 30, 40);

    for zoom in ZOOMS {
        let (mut hd_frame, anchor) = canvas(RenderScale::PixelPerfect);
        let (mut original, original_anchor) = canvas(RenderScale::PixelPerfect);

        let hd_stats = hd.draw_frame_silhouette_scaled(&mut hd_frame, index, anchor, zoom, color);
        let original_stats = renderer.draw_frame_silhouette_scaled(
            &mut original,
            frame,
            original_anchor,
            zoom,
            color,
        );

        assert_eq!(anchor, original_anchor);
        assert_eq!(hd_stats, original_stats, "zoom {zoom}");
        assert_eq!(hd_frame, original, "S = 1 silhouette, zoom {zoom}");
    }
}

// ---------------------------------------------------------------------------
// 3. Geometry and the documented sampling at every zoom
// ---------------------------------------------------------------------------

/// Side of the canvas used by the zoom sweep: the HD frame at the largest zoom
/// (`128 · 4`) still fits around the anchor.
const ZOOM_CANVAS: i32 = 1024;
const ZOOM_ANCHOR: i32 = 512;

/// Bounding box of the painted pixels `(ulx, uly, lrx, lry)`; `None` when the
/// buffer is empty.
fn painted_bounds(buffer: &RgbaBuffer) -> Option<(i32, i32, i32, i32)> {
    let mut bounds: Option<(i32, i32, i32, i32)> = None;

    for y in 0..buffer.height() as i32 {
        for x in 0..buffer.width() as i32 {
            if buffer.get(x, y).is_none_or(Rgba::is_transparent) {
                continue;
            }

            bounds = Some(match bounds {
                None => (x, y, x + 1, y + 1),
                Some((ulx, uly, lrx, lry)) => {
                    (ulx.min(x), uly.min(y), lrx.max(x + 1), lry.max(y + 1))
                }
            });
        }
    }

    bounds
}

#[test]
fn hd_frame_occupies_the_original_rect_at_the_hd_zoom() {
    let (sheet, shadows) = tank();
    let index = frame_with_shadow(&sheet, &shadows);
    let frame = sheet.frame(index).unwrap();
    let palette = ramp_palette();
    let brightness = BrightnessLut::build(&palette);
    let renderer = SpriteRenderer::neutral(&palette, &brightness);

    for render_scale in [RenderScale::Hd2, RenderScale::Hd4] {
        let factor = render_scale.scale();
        let hd_frame = upscaled_frame(frame, factor);
        let bank = bank_with(
            &sheet,
            &shadows,
            render_scale,
            index,
            Some(hd_frame.clone()),
            None,
        );
        let hd = HdSpriteRenderer::new(&bank, &palette, &brightness, HdRecolorState::default());
        let texture = hd_frame.recolor(hd.lut());
        let mut cache = HdSpriteCache::new();
        let side = ZOOM_CANVAS as u32;

        for zoom in ZOOMS {
            // The world scale of the original at `zoom` is `zoom · S` in the
            // HD frame: both pictures are drawn at that render zoom.
            let hd_zoom = zoom * factor;
            let mut hd_canvas = RgbaBuffer::filled(side, side, Rgba::TRANSPARENT);
            let mut original_canvas = RgbaBuffer::filled(side, side, Rgba::TRANSPARENT);

            hd.draw_frame_scaled(
                &mut hd_canvas,
                &mut cache,
                index,
                (ZOOM_ANCHOR, ZOOM_ANCHOR),
                hd_zoom,
            );
            renderer.draw_frame_scaled(
                &mut original_canvas,
                frame,
                (ZOOM_ANCHOR, ZOOM_ANCHOR),
                hd_zoom,
            );

            let hd_bounds = painted_bounds(&hd_canvas).expect("the HD frame must paint pixels");
            let original_bounds =
                painted_bounds(&original_canvas).expect("the original frame must paint pixels");

            // The hot spot is the source hot spot scaled by the asset ratio, so
            // both frames cover exactly the same rectangle at the same zoom.
            assert_eq!(
                hd_bounds, original_bounds,
                "the HD frame must occupy the original rect at the HD zoom \
                 ({render_scale:?}, zoom {zoom} → {hd_zoom})"
            );

            // The documented nearest mapping `destination · 64 · S / zoom` over
            // the HD source, re-derived here.
            let (width, height) = hd_frame.render_size(hd_zoom, factor);
            let (ulx, uly) = hd_frame.render_origin((ZOOM_ANCHOR, ZOOM_ANCHOR), hd_zoom, factor);

            assert_eq!((ulx, uly), (hd_bounds.0, hd_bounds.1), "origin");

            for y in 0..height {
                let source_y = (y * 64 * factor as i32 / hd_zoom as i32)
                    .clamp(0, hd_frame.height() as i32 - 1);

                for x in 0..width {
                    let source_x = (x * 64 * factor as i32 / hd_zoom as i32)
                        .clamp(0, hd_frame.width() as i32 - 1);
                    let expected = texture.texel(source_x as u32, source_y as u32).unwrap();
                    let actual = hd_canvas.get(ulx + x, uly + y).unwrap();

                    if expected.is_transparent() {
                        assert_eq!(
                            actual,
                            Rgba::TRANSPARENT,
                            "transparent HD texel must not be written \
                             ({render_scale:?}, zoom {zoom}, ({x}, {y}))"
                        );
                    } else {
                        assert_eq!(
                            actual, expected,
                            "HD sampling must follow `destination·64·S/zoom` \
                             ({render_scale:?}, zoom {zoom}, ({x}, {y}))"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn hd_shadow_matches_the_original_darkening() {
    let (sheet, shadows) = tank();
    let index = frame_with_shadow(&sheet, &shadows);
    let shadow = shadows.frame(index).unwrap();
    let palette = ramp_palette();
    let brightness = BrightnessLut::build(&palette);

    for render_scale in [RenderScale::Hd2, RenderScale::Hd4] {
        let factor = render_scale.scale();
        let bank = bank_with(
            &sheet,
            &shadows,
            render_scale,
            index,
            None,
            Some(upscaled_shadow(shadow, factor)),
        );
        let hd = HdSpriteRenderer::new(&bank, &palette, &brightness, HdRecolorState::default());
        let (mut hd_frame, anchor) = canvas(render_scale);
        hd_frame.fill(Rgba::rgb(210, 140, 70));

        let stats = hd.draw_shadow_scaled(&mut hd_frame, index, anchor, NATIVE_ZOOM * factor);

        // Reference: the original shadow on the same base color, upscaled ×S.
        let renderer = SpriteRenderer::neutral(&palette, &brightness);
        let (mut logical, logical_anchor) = canvas(RenderScale::PixelPerfect);
        logical.fill(Rgba::rgb(210, 140, 70));
        let reference_stats =
            renderer.draw_shadow_scaled(&mut logical, shadow, logical_anchor, NATIVE_ZOOM);
        let reference = upscale_nearest(&logical, CANVAS as u32 * factor, CANVAS as u32 * factor);

        assert!(stats.drawn > 0, "the TANK shadow must darken pixels");
        assert_eq!(
            stats.drawn,
            reference_stats.drawn * factor * factor,
            "every darkened pixel becomes an ×{factor} block ({render_scale:?})"
        );
        assert_eq!(
            hd_frame, reference,
            "the HD shadow must be the ×S upscale of the 1:1 darkening ({render_scale:?})"
        );
        assert_eq!(
            hd_frame.get(anchor.0, anchor.1),
            Some(Rgba::rgb(210, 140, 70).darken(SHADOW_BRIGHTNESS_LEVEL)),
            "full mask coverage is exactly Rgba::darken(5)"
        );
    }
}

#[test]
fn partial_shadow_mask_blends_the_darkening() {
    let (sheet, shadows) = tank();
    let index = frame_with_shadow(&sheet, &shadows);
    let shadow = shadows.frame(index).unwrap();
    let palette = ramp_palette();
    let brightness = BrightnessLut::build(&palette);

    // A uniform half-level mask: every darkened texel blends 50/50.
    let factor = 2;
    let width = u32::from(shadow.width) * factor;
    let height = u32::from(shadow.height) * factor;
    let level = 128u8;
    let mask = HdIndexMap::from_indices(
        width,
        height,
        vec![level; (width * height) as usize],
        vec![u8::MAX; (width * height) as usize],
    )
    .unwrap();
    let hd_shadow = HdShadowFrame::new(
        mask,
        scaled_hot(shadow.hotx, shadow.width, width),
        scaled_hot(shadow.hoty, shadow.height, height),
    );
    let bank = bank_with(
        &sheet,
        &shadows,
        RenderScale::Hd2,
        index,
        None,
        Some(hd_shadow),
    );
    let hd = HdSpriteRenderer::new(&bank, &palette, &brightness, HdRecolorState::default());
    let (mut frame, anchor) = canvas(RenderScale::Hd2);
    let base = Rgba::rgb(200, 100, 40);
    frame.fill(base);

    hd.draw_shadow_scaled(&mut frame, index, anchor, NATIVE_ZOOM * 2);

    let expected = base.mix(base.darken(SHADOW_BRIGHTNESS_LEVEL), level);
    assert_eq!(frame.get(anchor.0, anchor.1), Some(expected));
    assert_ne!(expected, base.darken(SHADOW_BRIGHTNESS_LEVEL));
    assert_ne!(expected, base);
}

// ---------------------------------------------------------------------------
// 4. Per-asset fallback
// ---------------------------------------------------------------------------

#[test]
fn fallback_without_hd_assets_is_the_scaled_original() {
    let (sheet, shadows) = tank();
    let index = frame_with_shadow(&sheet, &shadows);
    let frame = sheet.frame(index).unwrap();
    let shadow = shadows.frame(index).unwrap();
    let palette = ramp_palette();
    let brightness = BrightnessLut::build(&palette);

    for render_scale in [RenderScale::Hd2, RenderScale::Hd4] {
        // No manifest, no HD assets: a complete fallback.
        let cache = RemasterCache::open("__no_such_remaster_root__");
        let bank = HdSpriteBank::load(
            &cache,
            "TANK",
            sheet.clone(),
            Some(shadows.clone()),
            render_scale,
        );

        assert_eq!(bank.hd_frame_count(), 0);
        assert_eq!(bank.hd_shadow_count(), 0);

        let factor = render_scale.scale();
        let hd = HdSpriteRenderer::new(
            &bank,
            &palette,
            &brightness,
            HdRecolorState::new(TeamColors::Red, 4),
        );
        let snapshot = rotate_palette(&palette, &ColorCycles::new());
        let renderer = SpriteRenderer::new(&snapshot, &brightness, TeamColors::Red.lut(), 4);

        for zoom in [NATIVE_ZOOM, NATIVE_ZOOM * 2] {
            let mut hd_cache = HdSpriteCache::new();
            let (mut hd_frame, anchor) = canvas(render_scale);
            let (mut logical, logical_anchor) = canvas(RenderScale::PixelPerfect);

            hd.draw_unit_scaled(
                &mut hd_frame,
                &mut hd_cache,
                index,
                anchor,
                SHADOW_OFFSET,
                zoom * factor,
            );
            renderer.draw_unit_scaled(
                &mut logical,
                frame,
                Some(shadow),
                logical_anchor,
                SHADOW_OFFSET,
                zoom,
            );

            let reference =
                upscale_nearest(&logical, CANVAS as u32 * factor, CANVAS as u32 * factor);
            assert_eq!(
                hd_frame, reference,
                "a frame without an HD asset must be the ×S upscale of the original \
                 ({render_scale:?}, zoom {zoom})"
            );
        }
    }
}

#[test]
fn substitution_is_per_asset() {
    let (sheet, shadows) = tank();
    let index = frame_with_shadow(&sheet, &shadows);
    let other = (index + 1) % sheet.len();
    let frame = sheet.frame(index).unwrap();
    let palette = ramp_palette();
    let brightness = BrightnessLut::build(&palette);

    // Frame `index` gets an HD map with a shifted payload (so it must differ
    // from the fallback), frame `other` keeps the original path.
    let factor = 2;
    let width = u32::from(frame.width) * factor;
    let height = u32::from(frame.height) * factor;
    let mut payload = vec![0u8; (width * height) as usize];
    let mut coverage = vec![0u8; (width * height) as usize];

    for y in 0..height {
        for x in 0..width {
            if let Some(source) = frame.pixel(x / factor, y / factor) {
                let texel = (y * width + x) as usize;
                payload[texel] = source.wrapping_add(1);
                coverage[texel] = u8::MAX;
            }
        }
    }

    let hd_frame = HdIndexFrame::new(
        HdIndexMap::from_indices(width, height, payload, coverage).unwrap(),
        scaled_hot(frame.hotx, frame.width, width),
        scaled_hot(frame.hoty, frame.height, height),
    );
    let bank = bank_with(
        &sheet,
        &shadows,
        RenderScale::Hd2,
        index,
        Some(hd_frame),
        None,
    );
    assert_eq!(bank.hd_frame_count(), 1);
    assert!(bank.hd_frame(index).is_some());
    assert!(bank.hd_frame(other).is_none());

    let hd = HdSpriteRenderer::new(&bank, &palette, &brightness, HdRecolorState::default());
    // The reference uses the same recolor state as the HD renderer (default is
    // the gray team), otherwise the comparison would mix two palettes.
    let renderer = SpriteRenderer::new(
        &palette,
        &brightness,
        TeamColors::Gray.lut(),
        FULL_BRIGHTNESS_LEVEL,
    );
    let mut cache = HdSpriteCache::new();

    let (mut hd_drawn, anchor) = canvas(RenderScale::Hd2);
    hd.draw_frame_scaled(&mut hd_drawn, &mut cache, index, anchor, NATIVE_ZOOM * 2);

    let (mut hd_fallback, fallback_anchor) = canvas(RenderScale::Hd2);
    hd.draw_frame_scaled(
        &mut hd_fallback,
        &mut cache,
        other,
        fallback_anchor,
        NATIVE_ZOOM * 2,
    );

    let (mut logical, logical_anchor) = canvas(RenderScale::PixelPerfect);
    renderer.draw_frame_scaled(
        &mut logical,
        sheet.frame(other).unwrap(),
        logical_anchor,
        NATIVE_ZOOM,
    );
    let reference = upscale_nearest(&logical, CANVAS as u32 * 2, CANVAS as u32 * 2);

    assert_eq!(
        hd_fallback, reference,
        "the frame without an asset keeps the original pixels"
    );
    assert_ne!(
        hd_drawn, hd_fallback,
        "the frame with an asset comes from the HD index map"
    );
}

// ---------------------------------------------------------------------------
// 5. Silhouette (`zoom_level < 8`)
// ---------------------------------------------------------------------------

#[test]
fn silhouette_below_zoom_eight_is_the_scaled_team_colour_shape() {
    let (sheet, shadows) = tank();
    let index = frame_with_shadow(&sheet, &shadows);
    let frame = sheet.frame(index).unwrap();
    let palette = ramp_palette();
    let brightness = BrightnessLut::build(&palette);
    let color = Rgba::rgb(200, 30, 40);

    for render_scale in [RenderScale::Hd2, RenderScale::Hd4] {
        let factor = render_scale.scale();
        let bank = bank_with(
            &sheet,
            &shadows,
            render_scale,
            index,
            Some(upscaled_frame(frame, factor)),
            None,
        );
        let hd = HdSpriteRenderer::new(&bank, &palette, &brightness, HdRecolorState::default());
        let renderer = SpriteRenderer::neutral(&palette, &brightness);

        let (mut hd_frame, anchor) = canvas(render_scale);
        hd.draw_frame_silhouette_scaled(&mut hd_frame, index, anchor, NATIVE_ZOOM * factor, color);

        let (mut logical, logical_anchor) = canvas(RenderScale::PixelPerfect);
        renderer.draw_frame_silhouette_scaled(
            &mut logical,
            frame,
            logical_anchor,
            NATIVE_ZOOM,
            color,
        );
        let reference = upscale_nearest(&logical, CANVAS as u32 * factor, CANVAS as u32 * factor);

        assert_eq!(
            hd_frame, reference,
            "the silhouette must be the ×S upscale of the original shape ({render_scale:?})"
        );

        // Every painted pixel is the silhouette color — the palette LUT is not
        // consulted at all (`Gfx_TeamColorIndexBase` mode).
        for pixel in hd_frame.pixels().chunks_exact(4) {
            if pixel[3] == 0 {
                continue;
            }

            assert_eq!(
                [pixel[0], pixel[1], pixel[2], pixel[3]],
                color.to_rgba(),
                "silhouette pixels must be the team color, not a palette color"
            );
        }
    }

    assert!(is_silhouette_zoom(SILHOUETTE_ZOOM - 1));
    assert!(!is_silhouette_zoom(SILHOUETTE_ZOOM));
}

#[test]
fn silhouette_ignores_a_recolored_palette() {
    // The silhouette mode must not depend on the team LUT: a magenta-red team
    // color is painted even though the LUT would map every index elsewhere.
    let (sheet, shadows) = tank();
    let index = frame_with_shadow(&sheet, &shadows);
    let frame = sheet.frame(index).unwrap();
    let palette = ramp_palette();
    let brightness = BrightnessLut::build(&palette);
    let color = Rgba::rgb(1, 2, 3);
    let bank = bank_with(
        &sheet,
        &shadows,
        RenderScale::Hd2,
        index,
        Some(upscaled_frame(frame, 2)),
        None,
    );
    let hd = HdSpriteRenderer::new(
        &bank,
        &palette,
        &brightness,
        HdRecolorState::new(TeamColors::Blue, 0),
    );
    let mut cache = HdSpriteCache::new();
    let (mut body, anchor) = canvas(RenderScale::Hd2);
    hd.draw_frame_scaled(&mut body, &mut cache, index, anchor, NATIVE_ZOOM * 2);

    let (mut silhouette, silhouette_anchor) = canvas(RenderScale::Hd2);
    hd.draw_frame_silhouette_scaled(
        &mut silhouette,
        index,
        silhouette_anchor,
        NATIVE_ZOOM * 2,
        color,
    );

    let painted = silhouette
        .pixels()
        .chunks_exact(4)
        .filter(|pixel| pixel[3] != 0)
        .count();
    assert!(painted > 0, "the silhouette must cover the TANK frame");
    assert!(
        silhouette
            .pixels()
            .chunks_exact(4)
            .filter(|pixel| pixel[3] != 0)
            .all(|pixel| pixel[..3] == color.to_rgba()[..3]),
        "no palette color may leak into the silhouette"
    );
    // The shape is the same as the body's coverage.
    let covered = body
        .pixels()
        .chunks_exact(4)
        .filter(|pixel| pixel[3] != 0)
        .count();
    assert_eq!(
        painted, covered,
        "the silhouette covers exactly the body pixels"
    );
}

// ---------------------------------------------------------------------------
// 6. Loading through R1 with the R4 tag convention
// ---------------------------------------------------------------------------

#[test]
fn load_resolves_r4_tags_and_falls_back_per_asset() {
    let (sheet, shadows) = tank();
    let index = frame_with_shadow(&sheet, &shadows);
    let frame_index = u16::try_from(index).unwrap();
    // The R3 `.ktx2` fixture is a real 64×64 RGBA payload: it exercises the R1
    // transcode and the index-map transport, not the artwork.
    let manifest = format!(
        r#"{{"version":1,"assets":{{"{body}":{{"path":"terrain_tile_ETC1S.ktx2","scale":2}},"{shadow}":{{"path":"does_not_exist.ktx2","scale":2}}}}}}"#,
        body = sprite_frame_tag("TANK", frame_index, 2),
        shadow = sprite_shadow_tag("TANK", frame_index, 2),
    );
    let cache = RemasterCache::with_manifest(
        remaster_root(),
        RemasterManifest::parse(&manifest).expect("test manifest"),
    );
    let bank = HdSpriteBank::load(
        &cache,
        "TANK",
        sheet.clone(),
        Some(shadows),
        RenderScale::Hd2,
    );

    let loaded = bank
        .hd_frame(index)
        .expect("the listed .ktx2 must resolve into an index map");

    assert_eq!(loaded.width(), 64);
    assert_eq!(loaded.height(), 64);
    assert_eq!(
        bank.hd_frame_count(),
        1,
        "only the listed tag resolves; the other frames stay original"
    );
    assert_eq!(
        bank.hd_shadow_count(),
        0,
        "a listed tag with a missing file falls back"
    );
    assert_eq!(bank.frame_count(), sheet.len());
    assert_eq!(bank.scale(), RenderScale::Hd2);

    // The hot spot is the original hot spot scaled by the asset resolution.
    assert_eq!(
        loaded.hot_spot(),
        (
            scaled_hot(
                sheet.frame(index).unwrap().hotx,
                sheet.frame(index).unwrap().width,
                64
            ),
            scaled_hot(
                sheet.frame(index).unwrap().hoty,
                sheet.frame(index).unwrap().height,
                64
            )
        )
    );

    // Frames without an asset still draw the original pixels.
    let other = (index + 1) % sheet.len();
    assert!(bank.hd_frame(other).is_none());
}

// ---------------------------------------------------------------------------
// 7. Cache reuse inside a cycle frame
// ---------------------------------------------------------------------------

#[test]
fn cache_reuses_a_texture_inside_one_cycle_frame_and_invalidates_on_a_step() {
    let (sheet, shadows) = tank();
    let index = frame_with_shadow(&sheet, &shadows);
    let palette = ramp_palette();
    let brightness = BrightnessLut::build(&palette);
    let bank = bank_with(
        &sheet,
        &shadows,
        RenderScale::Hd2,
        index,
        Some(upscaled_frame(sheet.frame(index).unwrap(), 2)),
        None,
    );
    let mut cycles = ColorCycles::new();
    let mut cache = HdSpriteCache::new();

    for frame in 0..10 {
        // A unit is drawn once per frame; the palette cycles tick once a second
        // here, so nine of ten draws must be cache hits.
        let state = HdRecolorState::new(TeamColors::Red, 5).with_cycles(&cycles);
        let hd = HdSpriteRenderer::new(&bank, &palette, &brightness, state);
        let (mut target, anchor) = canvas(RenderScale::Hd2);

        hd.draw_frame_scaled(&mut target, &mut cache, index, anchor, NATIVE_ZOOM * 2);

        if frame == 0 {
            cycles.advance(1.0);
        }
    }

    assert_eq!(cache.misses(), 2, "a new cycle frame recolors once");
    assert_eq!(cache.hits(), 8, "the same cycle frame reuses the texture");
    assert_eq!(cache.len(), 2);
}

#[test]
fn hd_texture_of_a_frame_is_the_recolored_index_map() {
    let (sheet, shadows) = tank();
    let index = frame_with_shadow(&sheet, &shadows);
    let palette = ramp_palette();
    let brightness = BrightnessLut::build(&palette);
    let bank = bank_with(
        &sheet,
        &shadows,
        RenderScale::Hd2,
        index,
        Some(upscaled_frame(sheet.frame(index).unwrap(), 2)),
        None,
    );
    let hd = HdSpriteRenderer::new(
        &bank,
        &palette,
        &brightness,
        HdRecolorState::new(TeamColors::Gray, FULL_BRIGHTNESS_LEVEL),
    );
    let mut cache = HdSpriteCache::new();
    let texture: &HdSpriteTexture = hd.recolor_frame(&mut cache, index).expect("HD frame");

    let map = bank.hd_frame(index).unwrap().map();
    assert_eq!(texture.width(), map.width());
    assert_eq!(texture.height(), map.height());

    for y in 0..map.height() {
        for x in 0..map.width() {
            let expected = match map.coverage(x, y) {
                Some(0) | None => Rgba::TRANSPARENT,
                Some(coverage) => {
                    let color = hd.lut().color(map.payload(x, y).unwrap());
                    Rgba {
                        a: coverage,
                        ..color
                    }
                }
            };

            assert_eq!(texture.texel(x, y), Some(expected), "({x}, {y})");
        }
    }
}

#[test]
fn sprite_texture_can_be_built_without_the_renderer() {
    let (sheet, shadows) = tank();
    let index = frame_with_shadow(&sheet, &shadows);
    let frame = sheet.frame(index).unwrap();
    let palette = ramp_palette();
    let brightness = BrightnessLut::build(&palette);
    let hd_frame = upscaled_frame(frame, 2);
    let lut = IndexPaletteLut::new(
        &palette,
        &brightness,
        TeamColors::Red.lut(),
        FULL_BRIGHTNESS_LEVEL,
    );

    let texture = hd_frame.recolor(&lut);

    assert_eq!(texture.width(), u32::from(frame.width) * 2);
    assert_eq!(texture.height(), u32::from(frame.height) * 2);
    assert_eq!(
        texture.pixels().len(),
        texture.width() as usize * texture.height() as usize * 4
    );
    assert_eq!(hd_frame.map().recolor(&lut), texture);

    let first_painted = (0..texture.height())
        .flat_map(|y| (0..texture.width()).map(move |x| (x, y)))
        .find(|(x, y)| hd_frame.map().is_opaque(*x, *y))
        .expect("the TANK frame must have an opaque texel");

    assert_eq!(
        texture.texel(first_painted.0, first_painted.1),
        Some(
            lut.color(
                hd_frame
                    .map()
                    .payload(first_painted.0, first_painted.1)
                    .unwrap()
            )
        ),
        "a painted texel takes the LUT color"
    );
}
