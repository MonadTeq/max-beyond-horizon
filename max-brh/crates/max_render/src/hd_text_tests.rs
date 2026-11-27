//! Unit tests of the HD font adapter: the canonical atlas layout, the coverage
//! transport, per-font fallback and clipping. The public contract (metrics ×S,
//! the exact `×S` upscale of the 1:1 relief, the R1 tag path) is covered by
//! `tests/hd_text.rs`, per `max-brh-docs/development/test-organization.md`.

use super::*;

/// Canonical `×S` upscale of the original GNW bitmap as an HD atlas.
fn upscaled_atlas(id: GnwFontId, scale: RenderScale) -> HdGlyphAtlas {
    let font = HdFontAtlas::new(id, scale);
    let (width, height) = font.atlas_size();
    let mut coverage = vec![0u8; (width * height) as usize];

    for code in 0..=255u8 {
        let ch = char::from(code);
        let cell = font.glyph_rect(ch);

        for y in 0..cell.height {
            for x in 0..cell.width {
                if font.ink(ch, x, y) {
                    coverage[((cell.ulx as u32 + x as u32) + y as u32 * width) as usize] = 255;
                }
            }
        }
    }

    HdGlyphAtlas::from_coverage(width, height, coverage).expect("canonical atlas size")
}

#[test]
fn coverage_threshold_is_binary_and_outside_is_not_ink() {
    let atlas = HdGlyphAtlas::from_coverage(2, 2, vec![0, 127, 128, 255]).unwrap();

    assert_eq!(atlas.width(), 2);
    assert_eq!(atlas.height(), 2);
    assert!(!atlas.is_empty());
    assert!(!atlas.is_ink(0, 0), "coverage 0 is empty");
    assert!(!atlas.is_ink(1, 0), "127 is below the ink threshold");
    assert!(atlas.is_ink(0, 1), "128 is ink");
    assert!(atlas.is_ink(1, 1), "255 is ink");
    assert_eq!(atlas.coverage(1, 1), Some(255));
    assert_eq!(atlas.coverage(2, 1), None);
    assert!(!atlas.is_ink(2, 1), "outside the atlas is never ink");
    assert!(!atlas.is_ink(0, 2));
}

#[test]
fn malformed_atlases_are_rejected_without_panicking() {
    assert_eq!(HdGlyphAtlas::from_coverage(0, 4, vec![]), None);
    assert_eq!(HdGlyphAtlas::from_coverage(4, 0, vec![]), None);
    assert_eq!(HdGlyphAtlas::from_coverage(2, 2, vec![0; 3]), None);
    assert_eq!(HdGlyphAtlas::from_rgba(0, 2, &[]), None);
    assert_eq!(HdGlyphAtlas::from_rgba(2, 2, &[0; 15]), None);

    // The alpha channel is the coverage; RGB is ignored.
    let rgba = [9, 8, 7, 0, 9, 8, 7, 255, 9, 8, 7, 200, 9, 8, 7, 100];
    let atlas = HdGlyphAtlas::from_rgba(2, 2, &rgba).unwrap();
    assert!(!atlas.is_ink(0, 0));
    assert!(atlas.is_ink(1, 0));
    assert!(atlas.is_ink(0, 1));
    assert!(!atlas.is_ink(1, 1));
}

#[test]
fn atlas_layout_is_the_original_metrics_scaled_by_s() {
    for id in GnwFontId::ALL {
        for scale in [RenderScale::Hd2, RenderScale::Hd4] {
            let factor = scale.scale() as i32;
            let source = id.font();
            let font = HdFontAtlas::new(id, scale);
            let mut expected_width = 0u32;
            let mut cursor = 0;

            for code in 0..=255u8 {
                let ch = char::from(code);
                let expected_advance = source.char_width(ch) * factor;

                assert_eq!(font.advance(ch), expected_advance, "{id:?} {ch:?}");
                assert_eq!(
                    font.glyph_rect(ch),
                    PixelRect::new(cursor, 0, expected_advance, id.height() * factor),
                    "{id:?} {ch:?}: the glyphs are packed in character order"
                );

                cursor += expected_advance;
                expected_width += expected_advance as u32;
            }

            assert_eq!(font.glyph_count(), GLYPH_COUNT);
            assert_eq!(
                font.atlas_size(),
                (expected_width, id.height() as u32 * scale.scale())
            );
            assert!(
                expected_width <= 8192,
                "{id:?} {scale:?}: the one-row atlas must fit a 8192-texel texture"
            );
            assert_eq!(cursor as u32, expected_width);
            assert_eq!(
                font.text_width("M.A.X."),
                source.text_width("M.A.X.") * factor
            );
        }
    }
}

#[test]
fn atlas_must_cover_the_canonical_layout() {
    let id = GnwFontId::Font5;
    let scale = RenderScale::Hd2;
    let atlas = upscaled_atlas(id, scale);
    let (width, height) = (atlas.width(), atlas.height());

    assert!(HdFontAtlas::with_atlas(id, scale, atlas.clone()).is_some());

    // One column or one row short: rejected (the last glyphs would shift/clip).
    let narrow =
        HdGlyphAtlas::from_coverage(width - 1, height, vec![0; ((width - 1) * height) as usize])
            .unwrap();
    assert_eq!(HdFontAtlas::with_atlas(id, scale, narrow), None);

    let short =
        HdGlyphAtlas::from_coverage(width, height - 1, vec![0; (width * (height - 1)) as usize])
            .unwrap();
    assert_eq!(HdFontAtlas::with_atlas(id, scale, short), None);

    // A ×1 atlas requested at ×2 is too small: fallback instead of a squashed font.
    let times_one = upscaled_atlas(id, RenderScale::PixelPerfect);
    assert_eq!(HdFontAtlas::with_atlas(id, scale, times_one), None);
}

#[test]
fn block_padding_on_the_right_and_bottom_is_ignored() {
    let id = GnwFontId::Font2;
    let scale = RenderScale::Hd2;
    let atlas = upscaled_atlas(id, scale);
    let (width, height) = (atlas.width(), atlas.height());
    let padded_width = width + 3;
    let padded_height = height + 2;
    let mut coverage = vec![0u8; (padded_width * padded_height) as usize];

    for y in 0..height {
        let source = &atlas.coverage[(y * width) as usize..((y + 1) * width) as usize];
        let target = (y * padded_width) as usize;
        coverage[target..target + width as usize].copy_from_slice(source);
    }

    let padded = HdGlyphAtlas::from_coverage(padded_width, padded_height, coverage).unwrap();
    let font = HdFontAtlas::with_atlas(id, scale, padded).expect("padding is tolerated");
    let reference = HdFontAtlas::with_atlas(id, scale, atlas).unwrap();

    for code in 0..=255u8 {
        let ch = char::from(code);
        let cell = font.glyph_rect(ch);

        for y in 0..cell.height {
            for x in 0..cell.width {
                assert_eq!(
                    font.ink(ch, x, y),
                    reference.ink(ch, x, y),
                    "{ch:?} ({x},{y}): the padding must not move a glyph"
                );
            }
        }
    }
}

#[test]
fn ink_samples_the_atlas_cell_not_the_fallback_bitmap() {
    let id = GnwFontId::Font5;
    let scale = RenderScale::Hd2;
    let reference = HdFontAtlas::new(id, scale);
    let (width, height) = reference.atlas_size();
    // One marker texel inside the cell of 'A', at local (2, 3).
    let cell = reference.glyph_rect('A');
    let marker_x = cell.ulx as u32 + 2;
    let mut coverage = vec![0u8; (width * height) as usize];
    coverage[(marker_x + 3 * width) as usize] = 255;
    let atlas = HdGlyphAtlas::from_coverage(width, height, coverage).unwrap();
    let font = HdFontAtlas::with_atlas(id, scale, atlas).unwrap();

    assert!(font.has_atlas());
    assert!(font.ink('A', 2, 3), "the atlas texel is read from the cell");
    assert!(!font.ink('A', 3, 3));
    assert!(!font.ink('B', 2, 3), "the marker belongs to one cell only");
    assert!(!font.ink('A', -1, 3));
    assert!(!font.ink('A', 2, -1));
    assert!(!font.ink('A', cell.width, 3));
}

#[test]
fn ink_falls_back_to_the_original_bitmap_upscaled_by_s() {
    for id in GnwFontId::ALL {
        for scale in [RenderScale::Hd2, RenderScale::Hd4] {
            let font = HdFontAtlas::new(id, scale);
            let factor = scale.scale() as i32;

            assert!(!font.has_atlas());

            for ch in ['M', 'A', '.', 'X', '0', '?'] {
                let cell = font.glyph_rect(ch);

                for y in 0..cell.height {
                    for x in 0..cell.width {
                        assert_eq!(
                            font.ink(ch, x, y),
                            id.font().glyph_ink(ch, x / factor, y / factor),
                            "{id:?} {ch:?} ({x},{y}): nearest fallback"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn non_latin1_characters_use_the_fallback_cell() {
    let font = HdFontAtlas::new(GnwFontId::Font5, RenderScale::Hd2);

    assert_eq!(glyph_index('Ж'), GNW_FALLBACK as usize);
    assert_eq!(font.glyph_rect('Ж'), font.glyph_rect(GNW_FALLBACK));
    assert_eq!(font.advance('Ж'), font.advance(GNW_FALLBACK));
    assert!(!font.ink('Ж', 1000, 0), "outside the cell is never ink");
}

#[test]
fn render_point_scales_the_logical_frame() {
    let font = HdFontAtlas::new(GnwFontId::Font5, RenderScale::Hd4);

    assert_eq!(font.render_point(0, 0), (0, 0));
    assert_eq!(font.render_point(160, 478), (640, 1912));
    assert_eq!(
        HdFontAtlas::new(GnwFontId::Font5, RenderScale::PixelPerfect).render_point(160, 478),
        (160, 478),
        "the pixel-perfect mode is the identity"
    );
}

#[test]
fn fonts_bank_loads_nothing_without_a_remaster_root() {
    let cache = RemasterCache::open("__no_such_remaster_root__");
    let fonts = HdFonts::load(&cache, RenderScale::Hd2);

    assert_eq!(fonts.scale(), RenderScale::Hd2);
    assert_eq!(fonts.hd_count(), 0);
    assert!(!fonts.has_any());

    for id in GnwFontId::ALL {
        let font = fonts.font(id);

        assert_eq!(font.id(), id);
        assert!(!font.has_atlas());
        assert_eq!(font.atlas(), None);
    }
}

#[test]
fn pixel_perfect_never_loads_an_atlas() {
    let cache = RemasterCache::open("__no_such_remaster_root__");

    for id in GnwFontId::ALL {
        assert!(!HdFontAtlas::load(&cache, id, RenderScale::PixelPerfect).has_atlas());
    }

    let fonts = HdFonts::load(&cache, RenderScale::PixelPerfect);
    assert_eq!(fonts.hd_count(), 0);
}

#[test]
fn drawing_clips_without_panicking() {
    let font = HdFontAtlas::new(GnwFontId::Font5, RenderScale::Hd2);
    let width = font.text_width("M.A.X.");
    let mut buffer = RgbaBuffer::new(16, 12);

    assert_eq!(
        draw_text_hd(&mut buffer, -40, -40, &font, "M.A.X.", Rgba::WHITE),
        width
    );
    assert_eq!(
        draw_text_hd(&mut buffer, 100, 100, &font, "M.A.X.", Rgba::WHITE),
        width
    );
    assert_eq!(draw_text_hd(&mut buffer, 0, 0, &font, "", Rgba::WHITE), 0);
    draw_text_hd_shadowed(
        &mut buffer,
        -2,
        -2,
        &font,
        "End Turn",
        Rgba::WHITE,
        Rgba::rgb(1, 2, 3),
        Rgba::rgb(4, 5, 6),
    );
    draw_text_hd_outlined(
        &mut buffer,
        12,
        8,
        &font,
        "0",
        Rgba::WHITE,
        Rgba::rgb(1, 2, 3),
    );
    draw_text_line_hd(&mut buffer, -4, 0, RenderScale::Hd2, "M.A.X.", Rgba::WHITE);

    assert!(
        buffer.pixels().chunks_exact(4).any(|pixel| pixel[3] != 0),
        "partially visible text still paints its visible pixels"
    );
}

#[test]
fn line_width_hd_is_the_8x8_width_times_s() {
    for scale in [
        RenderScale::PixelPerfect,
        RenderScale::Hd2,
        RenderScale::Hd4,
    ] {
        assert_eq!(
            line_width_hd("M.A.X.", scale),
            crate::text::line_width("M.A.X.") * scale.scale() as i32
        );
        assert_eq!(line_width_hd("", scale), 0);
    }
}
