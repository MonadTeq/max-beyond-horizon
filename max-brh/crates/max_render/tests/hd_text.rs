//! Integration tests of the R6 HD raster fonts through the public API
//! (`max-brh-docs/development/test-organization.md`: unit → `src/`,
//! integration → `tests/`).
//!
//! The R1 path is exercised twice:
//!
//! * with the real `.ktx2` fixture
//!   (`max_assets/tests/data/remaster/terrain_tile_ETC1S.ktx2`, a 64×64 RGBA8
//!   texture) to prove the `font_<FONT>_x<S>` tag convention, the decode and the
//!   **size validation** — a texture of the wrong resolution (here also a
//!   listed-but-missing file and an absent tag) must fall back per font, never
//!   panic;
//! * with `tests/data/font_FONT2_x2.uastc.ktx2` — the canonical `×2` upscale of
//!   the embedded `FONT2.FON`, encoded by the R2 `remaster-encode` tool (UASTC,
//!   one thread) — to prove that a real HD atlas resolves, survives the
//!   transcode at the `INK_THRESHOLD` mask threshold and draws the exact `×S`
//!   upscale of the 1:1 text.
//!
//! Glyph content in the remaining cases is supplied as an in-memory canonical
//! atlas built from the original `FONT*.FON` metrics through the public API.
//!
//! Properties checked here:
//!
//! 1. metrics (`advance`, cell height, line spacing, line width) are **exactly**
//!    `S` times the 1:1 values, for every GNW font and both HD scales;
//! 2. an HD atlas that is the exact `×S` upscale of the original bitmap draws
//!    exactly the `×S` nearest upscale of [`draw_text_gnw`] — and without an
//!    atlas the fallback does the same (per-font fallback);
//! 3. the `Text_TextLine` relief ([`draw_text_gnw_shadowed`]) and the
//!    `GNW_TEXT_OUTLINE` copy ([`draw_text_gnw_outlined`]) keep their exact
//!    structure at `×S` (the outline/shadow pixels are present, not flat);
//! 4. `PixelPerfect` (`S = 1`) is byte-identical to the current GNW path, so
//!    the golden 1:1 frames cannot drift; an injected atlas does not change
//!    that, and [`HdFontAtlas::load`] never resolves one at `S = 1`;
//! 5. the built-in 8×8 fallback ([`draw_text_line_hd`]) is the exact `×S`
//!    upscale of [`draw_text_line`].

use std::path::{Path, PathBuf};

use max_assets::remaster::{font_tag, RemasterCache, RemasterManifest};
use max_render::buffer::{Rgba, RgbaBuffer};
use max_render::frame_scale::upscale_nearest;
use max_render::hd_text::{
    draw_text_hd, draw_text_hd_outlined, draw_text_hd_shadowed, draw_text_line_hd, line_width_hd,
    HdFontAtlas, HdFonts, HdGlyphAtlas,
};
use max_render::render_scale::RenderScale;
use max_render::text::{
    draw_text_gnw, draw_text_gnw_outlined, draw_text_gnw_shadowed, draw_text_line, line_width,
    GnwFontId,
};

const TEXT: &str = "M.A.X.";

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

/// A cache with a synthetic R6 manifest over the real `.ktx2` fixtures.
fn cache_with(entries: &[(&str, &str, u32)]) -> RemasterCache {
    let assets = entries
        .iter()
        .map(|(tag, file, scale)| format!(r#""{tag}":{{"path":"{file}","scale":{scale}}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let manifest = RemasterManifest::parse(&format!(r#"{{"version":1,"assets":{{{assets}}}}}"#))
        .expect("test manifest");

    RemasterCache::with_manifest(remaster_root(), manifest)
}

/// The canonical `×S` upscale of the original GNW bitmap, built through the
/// public API ([`HdFontAtlas::glyph_rect`] + [`HdFontAtlas::ink`]).
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

/// The buffer must be exactly the `×S` nearest upscale of the 1:1 one.
fn assert_upscale(one_to_one: &RgbaBuffer, hd: &RgbaBuffer, scale: RenderScale, what: &str) {
    let factor = scale.scale();
    let expected = upscale_nearest(
        one_to_one,
        one_to_one.width() * factor,
        one_to_one.height() * factor,
    );

    assert_eq!(hd.width(), one_to_one.width() * factor, "{what}: HD width");
    assert_eq!(
        hd.height(),
        one_to_one.height() * factor,
        "{what}: HD height"
    );
    assert_eq!(hd, &expected, "{what}: must be the exact ×S upscale");
}

fn count_color(buffer: &RgbaBuffer, color: Rgba) -> usize {
    buffer
        .pixels()
        .chunks_exact(4)
        .filter(|pixel| {
            pixel[0] == color.r && pixel[1] == color.g && pixel[2] == color.b && pixel[3] == color.a
        })
        .count()
}

// ---------------------------------------------------------------------------
// 1. Tags -> KTX2 -> size validation -> per-font fallback
// ---------------------------------------------------------------------------

#[test]
fn a_real_hd_atlas_loads_through_r1_and_draws_the_exact_upscale() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data");
    let manifest = RemasterManifest::parse(
        r#"{"version":1,"assets":{"font_FONT2_x2":{"path":"font_FONT2_x2.uastc.ktx2","scale":2}}}"#,
    )
    .expect("test manifest");
    let cache = RemasterCache::with_manifest(&root, manifest);
    let id = GnwFontId::Font2;
    let scale = RenderScale::Hd2;
    let source = id.font();
    let font = HdFontAtlas::load(&cache, id, scale);

    assert!(
        font.has_atlas(),
        "the canonical FONT2 ×2 atlas must resolve through R1"
    );
    let atlas = font.atlas().expect("resolved atlas");
    assert_eq!(font.atlas_size(), (3030, 18), "the FONT2 ×2 canonical size");
    assert_eq!((atlas.width(), atlas.height()), (3030, 18));
    assert!(atlas.width() < 8192, "a single-row atlas fits one texture");

    // The UASTC transcode of a stark mask must survive the ink threshold: every
    // glyph texel decodes to the same ink as the original bitmap at ×2.
    for code in 0..=255u8 {
        let ch = char::from(code);
        let cell = font.glyph_rect(ch);

        for y in 0..cell.height {
            for x in 0..cell.width {
                assert_eq!(
                    font.ink(ch, x, y),
                    source.glyph_ink(ch, x / 2, y / 2),
                    "{ch:?} ({x},{y}): the atlas mask must equal the upscaled bitmap"
                );
            }
        }
    }

    // And the drawing is exactly the ×S upscale of the 1:1 relief.
    let base = Rgba::rgb(200, 200, 200);
    let outline = Rgba::rgb(10, 20, 30);
    let shadow = Rgba::rgb(40, 50, 60);
    let text = "End Turn";
    let width = source.text_width(text) + 1;
    let height = source.height() + 1;
    let mut one_to_one = RgbaBuffer::new(width as u32, height as u32);
    let mut hd = RgbaBuffer::new((width * 2) as u32, (height * 2) as u32);

    draw_text_gnw_shadowed(&mut one_to_one, 0, 0, source, text, base, outline, shadow);
    draw_text_hd_shadowed(&mut hd, 0, 0, &font, text, base, outline, shadow);

    assert_upscale(&one_to_one, &hd, scale, "the real FONT2 ×2 atlas");

    // The same tag does not exist for ×4: the loader falls back per scale.
    let hd4 = HdFontAtlas::load(&cache, id, RenderScale::Hd4);
    assert!(!hd4.has_atlas());
    assert_eq!(hd4.text_width(text), source.text_width(text) * 4);
}

#[test]
fn font_tags_resolve_and_fall_back_per_font() {
    let font1_tag = font_tag("FONT1", 2);
    let font2_tag = font_tag("FONT2", 2);

    assert_eq!(font1_tag, "font_FONT1_x2");
    assert_eq!(font2_tag, "font_FONT2_x2");

    let cache = cache_with(&[
        // Listed and decodable, but 64×64 is far too small for the FONT1 metrics:
        // the size check must reject it (fallback), not draw a squashed font.
        (&font1_tag, "terrain_tile_ETC1S.ktx2", 2),
        // Listed, but the file does not exist: fallback.
        (&font2_tag, "does_not_exist.ktx2", 2),
        // FONT5 is not in the manifest at all: fallback.
    ]);
    let fonts = HdFonts::load(&cache, RenderScale::Hd2);

    assert_eq!(fonts.scale(), RenderScale::Hd2);
    assert_eq!(fonts.hd_count(), 0, "no font has a usable HD atlas");
    assert!(!fonts.has_any());

    for id in GnwFontId::ALL {
        let font = fonts.font(id);
        let source = id.font();

        assert_eq!(font.id(), id);
        assert!(!font.has_atlas());
        assert_eq!(font.atlas(), None);

        // The metrics still come from the original table, scaled by S.
        assert_eq!(font.text_width(TEXT), source.text_width(TEXT) * 2);
        assert_eq!(font.height(), source.height() * 2);
    }

    // The single-font loader behaves the same way.
    assert!(!HdFontAtlas::load(&cache, GnwFontId::Font1, RenderScale::Hd2).has_atlas());
    assert!(!HdFontAtlas::load(&cache, GnwFontId::Font5, RenderScale::Hd2).has_atlas());
}

#[test]
fn a_missing_manifest_is_a_full_per_font_fallback() {
    let cache = RemasterCache::open("__no_such_remaster_root__");
    let fonts = HdFonts::load(&cache, RenderScale::Hd4);

    assert_eq!(fonts.hd_count(), 0);

    for id in GnwFontId::ALL {
        assert!(!fonts.font(id).has_atlas());
    }

    // Fallback drawing still paints something.
    let font = fonts.font(GnwFontId::Font5);
    let mut buffer = RgbaBuffer::new(256, 64);
    let width = draw_text_hd(&mut buffer, 0, 0, font, "End Turn", Rgba::WHITE);

    assert_eq!(width, font.text_width("End Turn"));
    assert!(
        buffer.pixels().chunks_exact(4).any(|pixel| pixel[3] != 0),
        "the GNW fallback paints the upscaled original"
    );
}

// ---------------------------------------------------------------------------
// 2. Metrics: exactly ×S
// ---------------------------------------------------------------------------

#[test]
fn metrics_are_exactly_s_times_the_1to1_metrics() {
    let texts = [TEXT, "End Turn", "Hits 15/15", "044-034"];

    for id in GnwFontId::ALL {
        let source = id.font();

        for scale in [RenderScale::Hd2, RenderScale::Hd4] {
            let factor = scale.scale() as i32;
            let fallback = HdFontAtlas::new(id, scale);
            let hd = HdFontAtlas::with_atlas(id, scale, upscaled_atlas(id, scale))
                .expect("the canonical atlas is accepted");

            for font in [&fallback, &hd] {
                assert_eq!(font.height(), source.height() * factor, "{id:?} {scale:?}");
                assert_eq!(
                    font.line_height(),
                    source.line_height() * factor,
                    "{id:?} {scale:?}"
                );

                for ch in "M.A.X.end 15/".chars() {
                    assert_eq!(
                        font.advance(ch),
                        source.char_width(ch) * factor,
                        "{id:?} {scale:?} {ch:?}"
                    );
                }

                for text in texts {
                    assert_eq!(
                        font.text_width(text),
                        source.text_width(text) * factor,
                        "{id:?} {scale:?} {text:?}: line width"
                    );
                }
            }

            assert!(hd.has_atlas());
            assert_eq!(fallback.atlas(), None);
        }
    }
}

// ---------------------------------------------------------------------------
// 3. Drawing: exact ×S upscale of the 1:1 functions (atlas and fallback)
// ---------------------------------------------------------------------------

#[test]
fn hd_text_is_the_exact_upscale_of_the_1to1_line() {
    for id in GnwFontId::ALL {
        let source = id.font();
        let text = TEXT;

        for scale in [RenderScale::Hd2, RenderScale::Hd4] {
            let factor = scale.scale() as i32;
            let width = source.text_width(text) + 3;
            let height = source.height() + 3;
            let base = Rgba::rgb(200, 200, 200);

            let mut one_to_one = RgbaBuffer::new(width as u32, height as u32);
            let expected = draw_text_gnw(&mut one_to_one, 1, 1, source, text, base);
            assert_eq!(expected, source.text_width(text));

            let fallback = HdFontAtlas::new(id, scale);
            let hd_atlas = HdFontAtlas::with_atlas(id, scale, upscaled_atlas(id, scale))
                .expect("canonical atlas");

            for font in [&fallback, &hd_atlas] {
                let mut hd = RgbaBuffer::new((width * factor) as u32, (height * factor) as u32);
                let (x, y) = font.render_point(1, 1);
                let drawn = draw_text_hd(&mut hd, x, y, font, text, base);

                assert_eq!(drawn, expected * factor, "{id:?} {scale:?}: return width");
                assert_upscale(
                    &one_to_one,
                    &hd,
                    scale,
                    &format!("{id:?} {scale:?} plain text"),
                );
            }
        }
    }
}

#[test]
fn hd_relief_holds_for_every_printable_glyph() {
    // Every printable Latin-1 glyph: no off-by-one in the `+S` reserve or in the
    // shifted overwrite may survive, including glyphs whose last row/column is ink.
    let text: String = (32u8..=126).map(char::from).collect();
    let base = Rgba::rgb(200, 200, 200);
    let outline = Rgba::rgb(10, 20, 30);
    let shadow = Rgba::rgb(40, 50, 60);

    for id in GnwFontId::ALL {
        let source = id.font();
        // `draw_text_gnw_shadowed` wants `Text_GetWidth + 1` × `Text_GetHeight + 1`.
        let width = source.text_width(&text) + 1;
        let height = source.height() + 1;

        for scale in [RenderScale::Hd2, RenderScale::Hd4] {
            let factor = scale.scale() as i32;

            let mut one_plain = RgbaBuffer::new(width as u32, height as u32);
            let mut one_relief = RgbaBuffer::new(width as u32, height as u32);
            draw_text_gnw(&mut one_plain, 0, 0, source, &text, base);
            draw_text_gnw_shadowed(&mut one_relief, 0, 0, source, &text, base, outline, shadow);

            let fallback = HdFontAtlas::new(id, scale);
            let hd_atlas = HdFontAtlas::with_atlas(id, scale, upscaled_atlas(id, scale))
                .expect("canonical atlas");

            for font in [&fallback, &hd_atlas] {
                let mut hd_plain =
                    RgbaBuffer::new((width * factor) as u32, (height * factor) as u32);
                let mut hd_relief =
                    RgbaBuffer::new((width * factor) as u32, (height * factor) as u32);
                draw_text_hd(&mut hd_plain, 0, 0, font, &text, base);
                draw_text_hd_shadowed(&mut hd_relief, 0, 0, font, &text, base, outline, shadow);

                assert_upscale(
                    &one_plain,
                    &hd_plain,
                    scale,
                    &format!("{id:?} {scale:?} all printable glyphs"),
                );
                assert_upscale(
                    &one_relief,
                    &hd_relief,
                    scale,
                    &format!("{id:?} {scale:?} all printable glyphs relief"),
                );
            }
        }
    }
}

#[test]
fn hd_relief_is_the_exact_upscale_of_the_1to1_textline() {
    let base = Rgba::rgb(200, 200, 200);
    let outline = Rgba::rgb(10, 20, 30);
    let shadow = Rgba::rgb(40, 50, 60);

    for id in GnwFontId::ALL {
        let source = id.font();

        for scale in [RenderScale::Hd2, RenderScale::Hd4] {
            let factor = scale.scale() as i32;
            // `Text_GetWidth + 1` / `Text_GetHeight + 1` at 1:1.
            let width = source.text_width(TEXT) + 1;
            let height = source.height() + 1;

            let mut one_to_one = RgbaBuffer::new(width as u32, height as u32);
            draw_text_gnw_shadowed(&mut one_to_one, 0, 0, source, TEXT, base, outline, shadow);

            let fallback = HdFontAtlas::new(id, scale);
            let hd_atlas = HdFontAtlas::with_atlas(id, scale, upscaled_atlas(id, scale))
                .expect("canonical atlas");

            for font in [&fallback, &hd_atlas] {
                let mut hd = RgbaBuffer::new((width * factor) as u32, (height * factor) as u32);
                draw_text_hd_shadowed(&mut hd, 0, 0, font, TEXT, base, outline, shadow);

                assert_upscale(
                    &one_to_one,
                    &hd,
                    scale,
                    &format!("{id:?} {scale:?} Text_TextLine relief"),
                );

                // The relief is not flat: the stroke diagonal is the outline
                // and the pixel just outside the glyph is the shadow.
                assert!(
                    count_color(&hd, outline) > 0,
                    "{id:?} {scale:?}: the lower-right diagonal must be the outline"
                );
                assert!(
                    count_color(&hd, shadow) > 0,
                    "{id:?} {scale:?}: the pixel outside the glyph must be the shadow"
                );
                assert!(
                    count_color(&hd, base) > 0,
                    "{id:?} {scale:?}: the fill must survive"
                );
            }
        }
    }
}

#[test]
fn hd_outline_is_the_exact_upscale_of_the_1to1_outlined_text() {
    let base = Rgba::rgb(200, 200, 200);
    let outline = Rgba::rgb(10, 20, 30);

    for id in GnwFontId::ALL {
        let source = id.font();

        for scale in [RenderScale::Hd2, RenderScale::Hd4] {
            let factor = scale.scale() as i32;
            let width = source.text_width(TEXT) + 2;
            let height = source.height() + 2;

            let mut one_to_one = RgbaBuffer::new(width as u32, height as u32);
            draw_text_gnw_outlined(&mut one_to_one, 0, 0, source, TEXT, base, outline);

            let mut hd = RgbaBuffer::new((width * factor) as u32, (height * factor) as u32);
            let font = HdFontAtlas::with_atlas(id, scale, upscaled_atlas(id, scale))
                .expect("canonical atlas");
            draw_text_hd_outlined(&mut hd, 0, 0, &font, TEXT, base, outline);

            assert_upscale(
                &one_to_one,
                &hd,
                scale,
                &format!("{id:?} {scale:?} GNW_TEXT_OUTLINE"),
            );
            assert!(count_color(&hd, outline) > 0, "the outline copy is visible");
        }
    }
}

// ---------------------------------------------------------------------------
// 4. PixelPerfect: byte-identical to the current GNW path
// ---------------------------------------------------------------------------

#[test]
fn pixel_perfect_hd_font_is_byte_identical_to_gnw() {
    let cache = RemasterCache::open("__no_such_remaster_root__");
    let fonts = HdFonts::load(&cache, RenderScale::PixelPerfect);

    assert_eq!(fonts.hd_count(), 0, "S = 1 never loads an HD atlas");

    let base = Rgba::rgb(200, 200, 200);
    let outline = Rgba::rgb(10, 20, 30);
    let shadow = Rgba::rgb(40, 50, 60);

    for id in GnwFontId::ALL {
        let source = id.font();
        let text = "Hits 15/15";
        let width = source.text_width(text) + 2;
        let height = source.height() + 2;
        let font = fonts.font(id);

        assert!(
            !font.has_atlas(),
            "HdFonts::load at PixelPerfect resolves nothing"
        );

        // Also inject the canonical ×1 atlas: even then S = 1 must stay exact.
        let injected = HdFontAtlas::with_atlas(
            id,
            RenderScale::PixelPerfect,
            upscaled_atlas(id, RenderScale::PixelPerfect),
        )
        .expect("×1 atlas");

        for font in [font, &injected] {
            let mut one_to_one = RgbaBuffer::new(width as u32, height as u32);
            let mut hd = RgbaBuffer::new(width as u32, height as u32);

            draw_text_gnw(&mut one_to_one, 0, 0, source, text, base);
            draw_text_hd(&mut hd, 0, 0, font, text, base);
            assert_eq!(hd, one_to_one, "{id:?} plain S = 1");

            one_to_one.fill(Rgba::TRANSPARENT);
            hd.fill(Rgba::TRANSPARENT);

            draw_text_gnw_shadowed(&mut one_to_one, 0, 0, source, text, base, outline, shadow);
            draw_text_hd_shadowed(&mut hd, 0, 0, font, text, base, outline, shadow);
            assert_eq!(hd, one_to_one, "{id:?} relief S = 1");

            one_to_one.fill(Rgba::TRANSPARENT);
            hd.fill(Rgba::TRANSPARENT);

            draw_text_gnw_outlined(&mut one_to_one, 0, 0, source, text, base, outline);
            draw_text_hd_outlined(&mut hd, 0, 0, font, text, base, outline);
            assert_eq!(hd, one_to_one, "{id:?} outlined S = 1");
        }
    }
}

// ---------------------------------------------------------------------------
// 5. 8×8 fallback: exact ×S upscale
// ---------------------------------------------------------------------------

#[test]
fn eight_by_eight_fallback_is_the_exact_upscale() {
    let base = Rgba::rgb(200, 200, 200);
    let text = "M.A.X. 15";

    for scale in [RenderScale::Hd2, RenderScale::Hd4] {
        let factor = scale.scale() as i32;
        let width = line_width(text) + 2;
        let height = 8 + 2;

        let mut one_to_one = RgbaBuffer::new(width as u32, height as u32);
        draw_text_line(&mut one_to_one, 1, 1, text, base);

        let mut hd = RgbaBuffer::new((width * factor) as u32, (height * factor) as u32);
        let drawn = draw_text_line_hd(&mut hd, factor, factor, scale, text, base);

        assert_eq!(drawn, line_width(text) * factor);
        assert_eq!(line_width_hd(text, scale), line_width(text) * factor);
        assert_upscale(&one_to_one, &hd, scale, &format!("{scale:?} 8×8 fallback"));
    }
}

// ---------------------------------------------------------------------------
// 6. Glyph placement in the atlas is the canonical one
// ---------------------------------------------------------------------------

#[test]
fn glyph_cells_are_contiguous_and_scale_with_the_metrics() {
    for id in GnwFontId::ALL {
        let source = id.font();

        for scale in [RenderScale::Hd2, RenderScale::Hd4] {
            let factor = scale.scale() as i32;
            let font = HdFontAtlas::with_atlas(id, scale, upscaled_atlas(id, scale))
                .expect("canonical atlas");
            let (atlas_width, atlas_height) = font.atlas_size();
            let mut cursor = 0;

            for code in 0..=255u8 {
                let ch = char::from(code);
                let cell = font.glyph_rect(ch);

                assert_eq!(cell.ulx, cursor, "{id:?} {ch:?}: packed in character order");
                assert_eq!(cell.uly, 0);
                assert_eq!(cell.width, source.char_width(ch) * factor);
                assert_eq!(cell.height, source.height() * factor);

                cursor += cell.width;
            }

            assert_eq!(cursor as u32, atlas_width, "the last cell ends the atlas");
            assert_eq!(atlas_height, source.height() as u32 * scale.scale());
        }
    }
}
