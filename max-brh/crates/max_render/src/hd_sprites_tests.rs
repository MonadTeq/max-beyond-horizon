//! Unit tests of the R4 HD sprite adapter: private geometry helpers, the
//! palette LUT identity with [`SpriteRenderer::color_for`], the transport
//! convention and the cache bookkeeping. The gate (HD `×S` equals the
//! `SpriteRenderer` path) lives in `tests/hd_sprites.rs` (public API), per
//! `max-brh-docs/development/test-organization.md`.

use super::*;
use crate::palette::{ColorCycle, RotateDirection, PALETTE_BYTES};
use crate::render_scale::RenderScale;
use crate::sprite_atlas::{scale_length, scale_pixels, source_coordinate};
use max_assets::sprite::{decode_shadow, decode_sprite};

/// Palette whose 6-bit color is `index % 64` on every channel (the same ramp
/// as `sprite_atlas_tests`): an 8-bit color of `(index % 64) * 4`.
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

fn brightness() -> BrightnessLut {
    BrightnessLut::build(&ramp_palette())
}

fn map(width: u32, height: u32, payload: &[u8], coverage: &[u8]) -> HdIndexMap {
    HdIndexMap::from_indices(width, height, payload.to_vec(), coverage.to_vec())
        .expect("valid index map")
}

/// One-frame sprite assembled from encoded rows (the format of `sprite.rs`).
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

/// 3×2 body: row 0 — indices 1, 2 after a skipped texel; row 1 — index 3.
fn sample_sheet() -> SpriteSheet {
    decode_sprite(&build_sprite(
        3,
        2,
        1,
        1,
        vec![vec![1, 2, 1, 2, 0xFF], vec![0, 1, 3, 0xFF]],
    ))
    .unwrap()
}

/// 3×2 shadow: row 0 — darkened texels 1 and 2; row 1 — texel 0.
fn sample_shadow() -> ShadowSheet {
    decode_shadow(&build_sprite(
        3,
        2,
        1,
        1,
        vec![vec![1, 2, 0xFF], vec![0, 1, 0xFF]],
    ))
    .unwrap()
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
        HdIndexMap::from_indices(width, height, payload, coverage).unwrap(),
        (i32::from(frame.hotx) * factor as i32) as i16,
        (i32::from(frame.hoty) * factor as i32) as i16,
    )
}

// ---------------------------------------------------------------------------
// Transport convention
// ---------------------------------------------------------------------------

#[test]
fn index_map_rejects_degenerate_textures() {
    assert!(HdIndexMap::from_rgba(0, 0, &[]).is_none(), "empty");
    assert!(
        HdIndexMap::from_rgba(1, 1, &[0, 0, 0]).is_none(),
        "a texel is four bytes"
    );
    assert!(
        HdIndexMap::from_indices(1, 1, vec![1], vec![]).is_none(),
        "coverage missing"
    );
    assert!(
        HdIndexMap::from_indices(0, 1, vec![], vec![]).is_none(),
        "no width"
    );
    assert!(HdIndexMap::from_opaque_indices(1, 1, vec![]).is_none());
}

#[test]
fn index_map_reads_payload_and_coverage_channels() {
    // R = payload, G/B are ignored, A = coverage.
    let rgba = [7, 200, 200, 255, 9, 1, 2, 0, 11, 0, 0, 128];
    let map = HdIndexMap::from_rgba(3, 1, &rgba).unwrap();

    assert_eq!(map.payload(0, 0), Some(7));
    assert_eq!(map.coverage(0, 0), Some(255));
    assert!(map.is_opaque(0, 0));

    assert_eq!(map.payload(1, 0), Some(9));
    assert_eq!(map.coverage(1, 0), Some(0));
    assert!(!map.is_opaque(1, 0), "alpha 0 is transparency");

    assert_eq!(map.payload(2, 0), Some(11));
    assert_eq!(map.coverage(2, 0), Some(128));
    assert!(map.is_opaque(2, 0), "partial coverage is still painted");

    assert_eq!(map.payload(3, 0), None);
    assert_eq!(map.coverage(0, 1), None);
}

#[test]
fn opaque_index_map_fills_the_coverage() {
    let map = HdIndexMap::from_opaque_indices(2, 2, vec![1, 2, 3, 4]).unwrap();

    for x in 0..2 {
        for y in 0..2 {
            assert_eq!(map.coverage(x, y), Some(255), "({x}, {y})");
            assert!(map.is_opaque(x, y));
        }
    }
}

// ---------------------------------------------------------------------------
// Palette LUT: exactly SpriteRenderer::color_for
// ---------------------------------------------------------------------------

#[test]
fn palette_lut_equals_color_for_for_every_team_and_brightness() {
    let palette = ramp_palette();
    let brightness = brightness();

    for team in TeamColors::ALL {
        for level in 0..=FULL_BRIGHTNESS_LEVEL {
            let lut = IndexPaletteLut::new(&palette, &brightness, team.lut(), level);
            let renderer = SpriteRenderer::new(&palette, &brightness, team.lut(), level);

            for index in 0..=u8::MAX {
                assert_eq!(
                    lut.color(index),
                    renderer.color_for(index),
                    "index {index}, team {team:?}, level {level}: the HD LUT must \
                     reproduce palette[brightness[team[index]]]"
                );
            }
        }
    }
}

#[test]
fn palette_lut_equals_color_for_on_the_cycle_snapshot() {
    let palette = ramp_palette();
    let brightness = brightness();
    let mut cycles = ColorCycles::new();
    cycles.advance(1.0);

    for team in TeamColors::ALL {
        let lut = IndexPaletteLut::with_cycles(&palette, &cycles, &brightness, team.lut(), 5);
        let snapshot = rotate_palette(&palette, &cycles);
        let renderer = SpriteRenderer::new(&snapshot, &brightness, team.lut(), 5);

        for index in 0..=u8::MAX {
            assert_eq!(
                lut.color(index),
                renderer.color_for(index),
                "index {index}, team {team:?}: cycles must apply to the palette, \
                 not to the index"
            );
        }
    }
}

#[test]
fn palette_lut_keeps_the_cycle_rotation_visible() {
    // A cycle range covers slot 9..12: rotating the palette must change the
    // LUT, otherwise the beacon/drill blink would be lost in the HD path.
    let palette = ramp_palette();
    let brightness = brightness();
    let mut cycles = ColorCycles::new();
    cycles.advance(1.0);

    let static_lut = IndexPaletteLut::with_cycles(
        &palette,
        &ColorCycles::new(),
        &brightness,
        ColorLut::identity(),
        FULL_BRIGHTNESS_LEVEL,
    );
    let animated_lut = IndexPaletteLut::with_cycles(
        &palette,
        &cycles,
        &brightness,
        ColorLut::identity(),
        FULL_BRIGHTNESS_LEVEL,
    );

    assert_ne!(
        static_lut.colors(),
        animated_lut.colors(),
        "the animated LUT must differ from the static one"
    );
}

#[test]
fn palette_lut_from_renderer_is_the_same_table() {
    let palette = ramp_palette();
    let brightness = brightness();
    let renderer = SpriteRenderer::new(
        &palette,
        &brightness,
        TeamColors::Blue.lut(),
        SHADOW_BRIGHTNESS_LEVEL,
    );

    assert_eq!(
        IndexPaletteLut::from_renderer(&renderer),
        IndexPaletteLut::new(
            &palette,
            &brightness,
            TeamColors::Blue.lut(),
            SHADOW_BRIGHTNESS_LEVEL
        )
    );
}

// ---------------------------------------------------------------------------
// Recolored texture
// ---------------------------------------------------------------------------

#[test]
fn recolor_paints_covered_texels_and_keeps_transparency() {
    let palette = ramp_palette();
    let brightness = brightness();
    let lut = IndexPaletteLut::new(
        &palette,
        &brightness,
        TeamColors::Red.lut(),
        FULL_BRIGHTNESS_LEVEL,
    );
    // Payload 34/35 are team slots 32–39; coverage varies.
    let map = map(3, 1, &[34, 35, 36], &[255, 128, 0]);
    let texture = map.recolor(&lut);

    assert_eq!(texture.width(), 3);
    assert_eq!(texture.height(), 1);
    // Red team: slot 34 → 58, 35 → 59.
    assert_eq!(texture.texel(0, 0), Some(palette.rgba(58)));
    assert_eq!(texture.texel(0, 0).unwrap().a, 255);
    // Partial coverage keeps its alpha but takes the LUT color.
    let partial = texture.texel(1, 0).unwrap();
    assert_eq!((partial.r, partial.g, partial.b), {
        let color = palette.rgba(59);
        (color.r, color.g, color.b)
    });
    assert_eq!(partial.a, 128);
    // Coverage 0 — fully transparent, no palette lookup.
    assert_eq!(texture.texel(2, 0), Some(Rgba::TRANSPARENT));
    assert_eq!(texture.texel(3, 0), None);
}

#[test]
fn recolor_of_a_hard_map_is_opaque() {
    let palette = ramp_palette();
    let brightness = brightness();
    let sheet = sample_sheet();
    let lut = IndexPaletteLut::new(
        &palette,
        &brightness,
        ColorLut::identity(),
        FULL_BRIGHTNESS_LEVEL,
    );
    let texture = upscaled_frame(sheet.frame(0).unwrap(), 2).recolor(&lut);

    for y in 0..texture.height() {
        for x in 0..texture.width() {
            assert!(
                texture.texel(x, y).unwrap().a == 0 || texture.texel(x, y).unwrap().a == 255,
                "a hard-edged asset stays binary at ({x}, {y})"
            );
        }
    }
    // The ×2 upscale keeps the source indices.
    assert_eq!(texture.texel(2, 0), Some(palette.rgba(1)));
    assert_eq!(texture.texel(4, 0), Some(palette.rgba(2)));
    assert_eq!(texture.texel(0, 2), Some(palette.rgba(3)));
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

#[test]
fn hd_geometry_matches_the_original_at_scale_one() {
    let sheet = sample_sheet();
    let source = sheet.frame(0).unwrap();
    let hd = upscaled_frame(source, 1);

    for zoom in [8u32, 16, 32, 64, 96, 128] {
        let (width, height) = hd.render_size(zoom, 1);
        assert_eq!(width, scale_length(source.width, zoom), "zoom {zoom}");
        assert_eq!(height, scale_length(source.height, zoom), "zoom {zoom}");

        let (ulx, uly) = hd.render_origin((100, 100), zoom, 1);
        assert_eq!(ulx, 100 - scale_pixels(i32::from(source.hotx), zoom));
        assert_eq!(uly, 100 - scale_pixels(i32::from(source.hoty), zoom));

        for destination in 0..width {
            assert_eq!(
                hd_source_texel(destination, zoom, 1, hd.width()),
                source_coordinate(destination, zoom, i32::from(source.width)),
                "zoom {zoom}, destination {destination}"
            );
        }
    }
}

#[test]
fn hd_geometry_divides_by_the_presentation_scale() {
    let sheet = sample_sheet();
    let source = sheet.frame(0).unwrap();

    for factor in [2u32, 4] {
        let hd = upscaled_frame(source, factor);
        // At the world scale of the original (logical zoom 64, i.e. 64·S in the
        // HD frame) the HD sprite covers the ×S upscale of the 1:1 sprite.
        let zoom = 64 * factor;
        let (width, height) = hd.render_size(zoom, factor);

        assert_eq!(width, i32::from(source.width) * factor as i32);
        assert_eq!(height, i32::from(source.height) * factor as i32);
        assert_eq!(
            hd.render_origin((0, 0), zoom, factor),
            (
                -i32::from(source.hotx) * factor as i32,
                -i32::from(source.hoty) * factor as i32
            )
        );
    }
}

#[test]
fn silhouette_zoom_threshold_follows_the_oracle() {
    // `zoom_level < 8` (unitinfo.cpp:3111).
    assert!(is_silhouette_zoom(0));
    assert!(is_silhouette_zoom(7));
    assert!(!is_silhouette_zoom(8));
    assert!(!is_silhouette_zoom(64));
    assert_eq!(SILHOUETTE_ZOOM, 8);
}

#[test]
fn scaled_offset_follows_the_renderer_rule() {
    // The shadow/turret offset is in source sprite pixels and is scaled by
    // `zoom / 64`, exactly as `SpriteRenderer::draw_unit_scaled` does it.
    assert_eq!(scaled_offset((-12, -8), 64), (-12, -8));
    assert_eq!(scaled_offset((-12, -8), 128), (-24, -16));
    assert_eq!(scaled_offset((-12, -8), 32), (-6, -4));
    assert_eq!(scaled_offset((5, 7), 64), (5, 7));
    assert_eq!(scaled_offset((0, 0), 8), (0, 0));
    assert_eq!(scaled_offset((-3, -2), 0), (0, 0), "zoom 0 is clamped to 1");
}

#[test]
fn hot_spot_scaling_saturates_and_handles_zero_width() {
    assert_eq!(scaled_hot(22, 44, 88), 44, "×2 keeps the anchor centred");
    assert_eq!(scaled_hot(-3, 44, 88), -6);
    assert_eq!(
        scaled_hot(22, 0, 88),
        22,
        "degenerate source keeps the value"
    );
    assert_eq!(scaled_hot(i16::MAX, 1, 1_000_000), i16::MAX, "saturated");
    assert_eq!(scaled_hot(i16::MIN, 1, 1_000_000), i16::MIN, "saturated");
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

#[test]
fn cache_is_keyed_by_team_brightness_and_cycle_frame() {
    let palette = ramp_palette();
    let brightness = brightness();
    let sheet = sample_sheet();
    let source = sheet.frame(0).unwrap();
    let bank = HdSpriteBank::with_frames(
        sheet.clone(),
        None,
        RenderScale::Hd2,
        vec![Some(upscaled_frame(source, 2))],
        vec![],
    );
    let mut cache = HdSpriteCache::new();

    let base = HdRecolorState::new(TeamColors::Red, FULL_BRIGHTNESS_LEVEL);
    let renderer = HdSpriteRenderer::new(&bank, &palette, &brightness, base);
    renderer.recolor_frame(&mut cache, 0).expect("HD frame");
    renderer.recolor_frame(&mut cache, 0).expect("HD frame");

    assert_eq!(cache.misses(), 1, "the first draw recolors");
    assert_eq!(cache.hits(), 1, "the second draw reuses the texture");
    assert_eq!(cache.len(), 1);

    // Another team — another texture.
    let other_team = HdSpriteRenderer::new(
        &bank,
        &palette,
        &brightness,
        HdRecolorState::new(TeamColors::Blue, FULL_BRIGHTNESS_LEVEL),
    );
    other_team.recolor_frame(&mut cache, 0);

    // Another brightness — another texture.
    let other_level = HdSpriteRenderer::new(
        &bank,
        &palette,
        &brightness,
        HdRecolorState::new(TeamColors::Red, 3),
    );
    other_level.recolor_frame(&mut cache, 0);

    // Another palette-cycle frame — another texture.
    let mut cycles = ColorCycles::new();
    cycles.advance(1.0);
    let other_cycle =
        HdSpriteRenderer::new(&bank, &palette, &brightness, base.with_cycles(&cycles));
    other_cycle.recolor_frame(&mut cache, 0);

    assert_eq!(cache.len(), 4, "every recolor state is a separate entry");
    assert_eq!(cache.misses(), 4);
    assert_eq!(cache.hits(), 1);

    let cached = cache
        .get(&renderer.key(0))
        .expect("the red full-brightness texture is cached");

    assert_eq!(cached.width(), bank.hd_frame(0).unwrap().width());
    assert_eq!(
        cached.texel(2, 0),
        Some(renderer.lut().color(1)),
        "the cached texture is the LUT-recolored index map (x = 2 is source x = 1)"
    );
}

#[test]
fn cache_is_bounded_and_evicts_the_oldest_entry() {
    let palette = ramp_palette();
    let brightness = brightness();
    let sheet = sample_sheet();
    let source = sheet.frame(0).unwrap();
    let bank = HdSpriteBank::with_frames(
        sheet.clone(),
        None,
        RenderScale::Hd2,
        vec![Some(upscaled_frame(source, 2))],
        vec![],
    );
    let mut cache = HdSpriteCache::with_limit(2);

    for level in [0u8, 1, 2] {
        let renderer = HdSpriteRenderer::new(
            &bank,
            &palette,
            &brightness,
            HdRecolorState::new(TeamColors::Red, level),
        );
        renderer.recolor_frame(&mut cache, 0).expect("HD frame");
    }

    assert_eq!(cache.len(), 2, "the limit is respected");
    assert_eq!(cache.limit(), 2);
    assert_eq!(cache.misses(), 3);

    // The oldest (level 0) was evicted, the two newest are present.
    let level_zero = HdSpriteRenderer::new(
        &bank,
        &palette,
        &brightness,
        HdRecolorState::new(TeamColors::Red, 0),
    );
    assert!(
        cache.get(&level_zero.key(0)).is_none(),
        "level 0 was evicted"
    );

    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.misses(), 3, "clear keeps the counters");

    assert_eq!(HdSpriteCache::with_limit(0).limit(), 1, "0 is clamped to 1");
}

#[test]
fn recolor_state_clamps_the_brightness_level() {
    assert_eq!(
        HdRecolorState::new(TeamColors::Red, 9).level(),
        FULL_BRIGHTNESS_LEVEL
    );
    assert_eq!(HdRecolorState::new(TeamColors::Red, 5).level(), 5);
    assert_eq!(HdRecolorState::default().brightness, FULL_BRIGHTNESS_LEVEL);
    assert_eq!(HdRecolorState::default().team, TeamColors::Gray);
    assert!(HdRecolorState::default().clock().is_static());
}

#[test]
fn cache_key_includes_the_frame_and_the_sprite_id() {
    let state = HdRecolorState::new(TeamColors::Red, 4);

    let a = HdSpriteKey::new(0, 0, state);
    let b = HdSpriteKey::new(0, 1, state);
    let c = HdSpriteKey::new(1, 0, state);
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_eq!(a.brightness, 4);
    assert_eq!(a.team, TeamColors::Red);
    assert_eq!(a.cycles, [0; COLOR_CYCLES.len()]);
}

// ---------------------------------------------------------------------------
// Bank and loader
// ---------------------------------------------------------------------------

#[test]
fn bank_without_assets_falls_back_and_aligns_the_vectors() {
    let sheet = sample_sheet();
    let bank = HdSpriteBank::new(sheet.clone(), Some(sample_shadow()), RenderScale::Hd2);

    assert_eq!(bank.hd_frame_count(), 0);
    assert_eq!(bank.hd_shadow_count(), 0);
    assert!(!bank.is_hd());
    assert_eq!(bank.frame_count(), sheet.len());
    assert!(bank.hd_frame(0).is_none());
    assert!(bank.hd_frame(99).is_none());

    // Extra frames are dropped, missing entries become `None`.
    let bank = HdSpriteBank::with_frames(
        sheet.clone(),
        None,
        RenderScale::Hd2,
        vec![
            Some(upscaled_frame(sheet.frame(0).unwrap(), 2)),
            None,
            Some(upscaled_frame(sheet.frame(0).unwrap(), 2)),
        ],
        vec![],
    );

    assert_eq!(bank.hd_frame_count(), 1);
    assert!(bank.hd_shadow(0).is_none());
    assert!(bank.is_hd());
}

#[test]
fn loader_reads_the_r4_tag_convention_and_falls_back_per_asset() {
    let sheet = sample_sheet();
    let shadow = sample_shadow();
    let root = std::path::Path::new("__no_such_remaster_root__");
    let cache = RemasterCache::with_manifest(
        root,
        max_assets::remaster::RemasterManifest::parse(
            r#"{"version":1,"assets":{"unit_sprite_GHOST_000_x2":{"path":"ghost.ktx2","scale":2}}}"#,
        )
        .unwrap(),
    );

    let bank = HdSpriteBank::load(
        &cache,
        "GHOST",
        sheet.clone(),
        Some(shadow),
        RenderScale::Hd2,
    );

    assert_eq!(bank.hd_frame_count(), 0, "the file is missing");
    assert_eq!(bank.hd_shadow_count(), 0);
    assert_eq!(bank.scale(), RenderScale::Hd2);
    assert_eq!(bank.source(), &sheet);
}

// ---------------------------------------------------------------------------
// Palette and cycle table landmarks
// ---------------------------------------------------------------------------

#[test]
fn color_cycle_table_matches_the_oracle_rows() {
    assert_eq!(
        COLOR_CYCLES[1],
        ColorCycle {
            start_index: 13,
            end_index: 16,
            direction: RotateDirection::Right,
            step_ms: 1000 / 6,
        }
    );
    assert_eq!(COLOR_CYCLES.len(), 11);
    assert_eq!(PALETTE_BYTES, PALETTE_SIZE * 3);
    assert_eq!(
        std::mem::size_of::<[u64; COLOR_CYCLES.len()]>(),
        88,
        "the cycle frame of the cache key is 11 counters"
    );
}
