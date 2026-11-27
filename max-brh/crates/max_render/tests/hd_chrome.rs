//! Integration tests of the R5 HD chrome and UI skins through the public API
//! (`max-brh-docs/development/test-organization.md`: unit → `src/`,
//! integration → `tests/`).
//!
//! The HD payload is the real R1/R3 `.ktx2` fixture
//! (`max_assets/tests/data/remaster/terrain_tile_ETC1S.ktx2`, a 64×64 RGBA8
//! texture) wrapped in the R5 manifest tags: it exercises the R1 transcode and
//! the tag convention; the artwork itself is irrelevant. Every expected pixel is
//! recomputed with an independent implementation of the documented equal-block
//! mapping.
//!
//! Properties checked here:
//!
//! 1. `chrome_<RES>_x<S>` tags resolve through R1, a missing/corrupt/absent
//!    asset falls back **per asset** (a listed-but-missing `.ktx2`, a tag the
//!    manifest does not list), and the HD frame/panel/button land on the R3
//!    equal-block mapping;
//! 2. a manifest `ui_<slot>_x<S>` texture substitutes the procedural
//!    `render_ui` box while widget rectangles and hit tests stay untouched;
//! 3. `PixelPerfect` never enters the HD layer: the 640×480 render is
//!    byte-identical to the plain `render_ui`/chrome path;
//! 4. layout stays in the logical 640×480 coordinates at every `S`: the
//!    presentation chain divides a click by `S`, and `HdChrome` only scales at
//!    output;
//! 5. the palette fallback ([`ChromeSource`]: the real `MAX.RES`
//!    `FRAMEPIC`/`PANELTOP`/button images) is the exact 1:1 picture at `S = 1`
//!    and the nearest `×S` upscale in the HD modes, while an HD asset replaces
//!    it as soon as it exists.

use std::path::{Path, PathBuf};

use max_assets::image::{BigImage, SimpleImage};
use max_assets::remaster::{chrome_image_tag, ui_skin_tag, RemasterCache, RemasterManifest};
use max_assets::ResArchive;
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::frame_scale::upscale_nearest;
use max_render::hd_chrome::{stretch_into, ChromePanel, ChromeSource, HdChrome, HdImage, HdUiSkin};
use max_render::layout;
use max_render::presentation::ScreenGeometry;
use max_render::render_scale::RenderScale;
use max_render::ui::{
    hit_test, render_ui, render_ui_with_skin, Button, ListBox, Point, Rect, TextBox, UiBox,
    UiState, Widget,
};

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

/// A cache with a synthetic R5 manifest over the real `.ktx2` fixtures.
fn cache_with(entries: &[(&str, &str)]) -> RemasterCache {
    let assets = entries
        .iter()
        .map(|(tag, file)| format!(r#""{tag}":{{"path":"{file}","scale":2}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let manifest = RemasterManifest::parse(&format!(r#"{{"version":1,"assets":{{{assets}}}}}"#))
        .expect("test manifest");

    RemasterCache::with_manifest(remaster_root(), manifest)
}

/// Independent reference implementation of the documented equal-block mapping
/// (`local · source / destination`), skipping fully transparent texels.
fn reference_equal_block(image: &HdImage, width: i32, height: i32) -> RgbaBuffer {
    let mut buffer = RgbaBuffer::new(width as u32, height as u32);

    for local_y in 0..height {
        let source_y = (local_y as u32 * image.height() / height as u32) as u32;

        for local_x in 0..width {
            let source_x = (local_x as u32 * image.width() / width as u32) as u32;
            let color = image.texel(source_x, source_y).expect("source texel");

            if !color.is_transparent() {
                buffer.set(local_x, local_y, color);
            }
        }
    }

    buffer
}

// ---------------------------------------------------------------------------
// 1. Tags -> KTX2 -> RGBA8, per-asset fallback
// ---------------------------------------------------------------------------

#[test]
fn chrome_loads_r5_tags_and_falls_back_per_asset() {
    let frame_tag = chrome_image_tag("FRAMEPIC", 2);
    let panel_tag = chrome_image_tag("PANELTOP", 2);
    let button_tag = chrome_image_tag("ENDTRN_U", 2);
    let cache = cache_with(&[
        (&frame_tag, "terrain_tile_ETC1S.ktx2"),
        // Listed, but the file does not exist: must fall back.
        (&panel_tag, "does_not_exist.ktx2"),
        (&button_tag, "terrain_tile_ETC1S.ktx2"),
    ]);
    // `MNUBTN` is not in the manifest at all and `PANELBTM` is only requested to
    // prove the absent tag stays absent.
    let chrome = HdChrome::load_with(
        &cache,
        RenderScale::Hd2,
        &["ENDTRN_U", "MNUBTN", "PANELBTM"],
    );

    assert_eq!(chrome.hd_count(), 2, "only the resolvable assets load");
    assert_eq!(chrome.scale(), RenderScale::Hd2);

    let frame = chrome.frame().expect("the FRAMEPIC tag must resolve");
    assert_eq!((frame.width(), frame.height()), (64, 64));
    assert!(
        chrome.panel_top().is_none(),
        "a listed tag with a missing file falls back"
    );
    assert!(
        chrome.panel_bottom().is_none(),
        "a tag absent from the manifest falls back"
    );
    assert!(chrome.image("ENDTRN_U").is_some());
    assert!(chrome.image("MNUBTN").is_none());

    // The HD frame is stretched onto the whole 1280×960 render frame with the
    // documented mapping.
    let mut hd_frame = RgbaBuffer::new(1280, 960);
    assert_eq!(chrome.draw_frame(&mut hd_frame), Some(1280 * 960));
    assert_eq!(
        hd_frame,
        reference_equal_block(frame, 1280, 960),
        "the HD frame must land on the R3 equal-block mapping"
    );

    // The panel has no HD asset: the drawer returns `None` and nothing is
    // painted, so the caller keeps the original palette `PANELTOP`.
    let mut panel = RgbaBuffer::new(342, 478);
    assert_eq!(chrome.draw_panel(&mut panel, ChromePanel::Top), None);
    assert_eq!(panel, RgbaBuffer::new(342, 478));

    // A named button image is stretched into its logical rectangle ×S.
    let button = chrome.image("ENDTRN_U").expect("resolved button");
    let mut hd_button = RgbaBuffer::new(144, 42);
    assert_eq!(
        chrome.render_rect(Rect::new(
            layout::END_TURN_BUTTON.ulx,
            layout::END_TURN_BUTTON.uly,
            layout::END_TURN_BUTTON.width,
            layout::END_TURN_BUTTON.height,
        )),
        PixelRect::new(780, 4, 144, 42),
        "the End Turn button at S = 2"
    );
    assert_eq!(
        chrome.draw_image(&mut hd_button, "ENDTRN_U", Rect::new(0, 0, 72, 21)),
        Some(144 * 42),
        "72×21 at S = 2 is 144×42"
    );
    assert_eq!(hd_button, reference_equal_block(button, 144, 42));
}

#[test]
fn a_missing_manifest_is_a_full_fallback() {
    let cache = RemasterCache::open("__no_such_remaster_root__");
    let chrome = HdChrome::load_with(&cache, RenderScale::Hd2, &["ENDTRN_U"]);
    let mut frame = RgbaBuffer::new(1280, 960);

    assert_eq!(chrome.hd_count(), 0);
    assert_eq!(chrome.draw_frame(&mut frame), None);
    assert_eq!(chrome.draw_panel(&mut frame, ChromePanel::Top), None);
    assert_eq!(
        chrome.draw_image(&mut frame, "ENDTRN_U", Rect::new(0, 0, 72, 21)),
        None
    );
    assert_eq!(frame, RgbaBuffer::new(1280, 960));

    let skin = HdUiSkin::load(&cache, RenderScale::Hd2);
    assert!(skin.is_empty());
}

// ---------------------------------------------------------------------------
// 2. UI box skins substitute, geometry and hit tests do not move
// ---------------------------------------------------------------------------

#[test]
fn hd_ui_skin_substitutes_the_button_box_from_a_manifest_asset() {
    let button_tag = ui_skin_tag("button", 2);
    let cache = cache_with(&[(&button_tag, "terrain_tile_ETC1S.ktx2")]);
    let skin = HdUiSkin::load(&cache, RenderScale::Hd2);

    assert_eq!(skin.part_count(), 1, "only the listed slot resolves");
    assert!(skin.part(UiBox::Button).is_some());
    assert!(skin.part(UiBox::Panel).is_none());

    let mut list = ListBox::with_items(
        Rect::new(0, 40, 64, 28),
        vec!["alpha".to_string(), "beta".to_string()],
    );
    list.select(1);
    let ui = UiState::new(vec![
        Widget::Button(Button::new("OK", Rect::new(0, 0, 32, 32))),
        Widget::TextBox(TextBox::with_value(
            "Name",
            "game",
            "hint",
            Rect::new(0, 36, 64, 4),
        )),
        Widget::ListBox(list),
    ]);
    let widgets_before = ui.widgets.clone();
    let hit_before = [
        hit_test(&ui, Point::new(16, 16)),
        hit_test(&ui, Point::new(16, 37)),
        hit_test(&ui, Point::new(5, 61)),
        hit_test(&ui, Point::new(63, 67)),
    ];

    let mut plain = RgbaBuffer::new(64, 68);
    let mut hd = RgbaBuffer::new(64, 68);
    render_ui(&mut plain, &ui);
    render_ui_with_skin(&mut hd, &ui, Some(&skin));

    assert_ne!(plain, hd, "the button box must come from the HD skin");

    // The button box is exactly the HD texture stretched to 32×32 (the caption
    // is drawn over it afterwards).
    let image = skin.part(UiBox::Button).expect("button skin");
    let box_only = reference_equal_block(image, 32, 32);
    for (x, y) in [(0, 0), (31, 0), (0, 31), (31, 31)] {
        assert_eq!(hd.get(x, y), box_only.get(x, y), "({x}, {y})");
    }

    // The field and the list have no HD slot: they keep the procedural theme.
    assert_eq!(hd.get(1, 37), Some(ui.theme.panel));
    assert_eq!(hd.get(1, 41), Some(ui.theme.panel));
    // The selected list row keeps the theme color (selection slot absent).
    assert_eq!(hd.get(2, 54), Some(ui.theme.selection));

    // Geometry and hit testing are read-only: the same logical rectangles and
    // the same widget indices as before rendering.
    assert_eq!(ui.widgets, widgets_before, "layout must not move");
    assert_eq!(
        hit_before,
        [
            hit_test(&ui, Point::new(16, 16)),
            hit_test(&ui, Point::new(16, 37)),
            hit_test(&ui, Point::new(5, 61)),
            hit_test(&ui, Point::new(63, 67)),
        ]
    );
    assert_eq!(hit_before[0], Some(0));
    assert_eq!(hit_before[1], Some(1));
    assert_eq!(hit_before[2], Some(2));

    // Hover/disabled fall back to the base button skin (no separate slot).
    let mut ui_hover = ui.clone();
    ui_hover.hover = Some(0);
    let mut hover = RgbaBuffer::new(64, 68);
    render_ui_with_skin(&mut hover, &ui_hover, Some(&skin));
    for (x, y) in [(0, 0), (31, 31)] {
        assert_eq!(
            hover.get(x, y),
            box_only.get(x, y),
            "the hover state reuses the button skin at ({x}, {y})"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. S = 1: the byte-for-byte path is untouched
// ---------------------------------------------------------------------------

#[test]
fn pixel_perfect_never_enters_the_hd_layer() {
    let cache = cache_with(&[
        (&chrome_image_tag("FRAMEPIC", 2), "terrain_tile_ETC1S.ktx2"),
        (&ui_skin_tag("button", 2), "terrain_tile_ETC1S.ktx2"),
    ]);

    // `load` itself refuses the pixel-perfect mode (a ×1 tag is never requested).
    let chrome = HdChrome::load(&cache, RenderScale::PixelPerfect);
    assert_eq!(chrome.hd_count(), 0);
    assert!(!chrome.is_hd());

    let mut frame = RgbaBuffer::new(640, 480);
    assert_eq!(chrome.draw_frame(&mut frame), None);
    assert_eq!(
        frame,
        RgbaBuffer::new(640, 480),
        "S = 1 keeps the original palette FRAMEPIC path"
    );

    let skin = HdUiSkin::load(&cache, RenderScale::PixelPerfect);
    assert!(skin.is_empty());

    let ui = UiState::new(vec![
        Widget::Button(Button::new("OK", Rect::new(0, 0, 40, 20))),
        Widget::TextBox(TextBox::new("Name", Rect::new(0, 22, 40, 20))),
    ]);
    let mut plain = RgbaBuffer::new(64, 64);
    let mut with_skin = RgbaBuffer::new(64, 64);
    render_ui(&mut plain, &ui);
    render_ui_with_skin(&mut with_skin, &ui, Some(&skin));

    assert_eq!(
        plain, with_skin,
        "the S = 1 render is byte-identical to render_ui (golden 1:1)"
    );
}

// ---------------------------------------------------------------------------
// 4. Layout and hit tests stay in the logical 640×480 space
// ---------------------------------------------------------------------------

#[test]
fn layout_and_hit_testing_stay_in_logical_coordinates_at_every_scale() {
    let points = [(0, 0), (179, 17), (180, 18), (627, 465), (639, 479)];

    for scale in [
        RenderScale::PixelPerfect,
        RenderScale::Hd2,
        RenderScale::Hd4,
    ] {
        let (render_width, render_height) = scale.render_size();
        let geometry = ScreenGeometry::native_with_render_scale(
            (render_width, render_height),
            (render_width, render_height),
            scale,
        );
        let factor = scale.scale() as i32;

        // A click at a logical point arrives at window point ·S and is divided
        // by S by the presentation chain: UI hit tests never see the HD space.
        for (x, y) in points {
            assert_eq!(
                geometry.window_to_frame(x * factor, y * factor),
                Some((x, y)),
                "{scale:?}: logical ({x}, {y})"
            );
        }

        let chrome = HdChrome::empty(scale);
        assert_eq!(
            chrome.frame_rect(),
            PixelRect::new(0, 0, render_width as i32, render_height as i32)
        );
        assert_eq!(
            chrome.render_rect(Rect::new(180, 18, 448, 448)),
            max_render::hd_chrome::scaled_pixel_rect(layout::MAIN_MAP, scale),
            "{scale:?}: only the output is scaled"
        );
    }

    // The layout constants themselves never move: the original 640×480 space.
    assert_eq!((layout::WINDOW_WIDTH, layout::WINDOW_HEIGHT), (640, 480));
    assert_eq!(layout::MAIN_MAP, PixelRect::new(180, 18, 448, 448));
    assert!(layout::MAIN_MAP.contains(180, 18));
    assert!(layout::MAIN_MAP.contains(627, 465));
    assert!(!layout::MAIN_MAP.contains(628, 465));
    assert_eq!(layout::END_TURN_BUTTON, PixelRect::new(390, 2, 72, 21));
}

// ---------------------------------------------------------------------------
// 5. Composition: frame -> panel -> button over it
// ---------------------------------------------------------------------------

#[test]
fn hd_chrome_composes_frame_panel_and_image_with_cut_outs() {
    let frame = HdImage::from_rgba(1, 1, vec![10, 20, 30, 255]).expect("frame");
    let panel = HdImage::from_rgba(1, 1, vec![40, 50, 60, 255]).expect("panel");
    // A 2×1 button: the left texel is a cut-out (A = 0), the right one is opaque.
    let button = HdImage::from_rgba(2, 1, vec![0, 0, 0, 0, 200, 210, 220, 255]).expect("button");
    let chrome = HdChrome::with_images(
        RenderScale::Hd2,
        Some(frame),
        Some(panel),
        None,
        [("ENDTRN_U".to_string(), button)].into_iter().collect(),
    );
    let mut dst = RgbaBuffer::new(342, 960);

    chrome.draw_frame(&mut dst).expect("HD frame");
    chrome
        .draw_panel(&mut dst, ChromePanel::Top)
        .expect("HD panel");
    chrome
        .draw_image(&mut dst, "ENDTRN_U", Rect::new(0, 0, 2, 1))
        .expect("HD button");

    assert_eq!(
        dst.get(0, 0),
        Some(Rgba::rgb(40, 50, 60)),
        "panel over frame"
    );
    assert_eq!(
        dst.get(0, 1),
        Some(Rgba::rgb(40, 50, 60)),
        "the cut-out texel keeps what is below it"
    );
    assert_eq!(
        dst.get(2, 0),
        Some(Rgba::rgb(200, 210, 220)),
        "the opaque texel is written"
    );
    assert_eq!(
        dst.get(4, 1),
        Some(Rgba::rgb(40, 50, 60)),
        "outside the button rectangle the panel remains"
    );
}

// ---------------------------------------------------------------------------
// 6. The public stretch helper matches the reference exactly
// ---------------------------------------------------------------------------

#[test]
fn stretch_into_is_the_reference_equal_block_mapping() {
    let cache = cache_with(&[(&chrome_image_tag("FRAMEPIC", 2), "terrain_tile_ETC1S.ktx2")]);
    let chrome = HdChrome::load(&cache, RenderScale::Hd2);
    let frame = chrome.frame().expect("frame");

    let mut dst = RgbaBuffer::new(120, 90);
    let rect = PixelRect::new(10, 5, 100, 80);

    assert_eq!(stretch_into(&mut dst, frame, rect), 100 * 80);

    let reference = reference_equal_block(frame, 100, 80);
    for y in 0..80i32 {
        for x in 0..100i32 {
            assert_eq!(
                dst.get(rect.ulx + x, rect.uly + y),
                reference.get(x, y),
                "({x}, {y})"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 7. The original palette chrome: per-asset fallback (`ChromeSource`)
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

/// The original palette chrome of the real `MAX.RES` (`FRAMEPIC`, the panel
/// halves and one button), the reference the fallback must reproduce.
fn original_chrome() -> ChromeSource {
    let path = repo_root().join("max-original").join("MAX.RES");
    assert!(
        path.is_file(),
        "required test fixture is missing: {}",
        path.display()
    );

    let archive = ResArchive::open(&path).expect("MAX.RES");
    let frame =
        BigImage::decode(archive.read("FRAMEPIC").expect("FRAMEPIC")).expect("FRAMEPIC decodes");
    let simple = |name: &str| {
        archive
            .read(name)
            .ok()
            .and_then(|bytes| SimpleImage::decode(bytes).ok())
    };
    let mut source = ChromeSource::with_panels(frame, simple("PANELTOP"), simple("PANELBTM"));

    if let Some(button) = simple("FIND_OFF") {
        source = source.with_image("FIND_OFF", button);
    }

    source
}

/// The 1:1 palette background, exactly the loop of `max_app::chrome::Chrome::draw`.
fn palette_frame(source: &ChromeSource) -> RgbaBuffer {
    let frame = source.frame();
    let mut buffer = RgbaBuffer::new(frame.width as u32, frame.height as u32);

    for y in 0..frame.height as u32 {
        for x in 0..frame.width as u32 {
            let rgb = frame.rgb(x, y).unwrap_or([0, 0, 0]);

            buffer.set(x as i32, y as i32, Rgba::from_rgb(rgb));
        }
    }

    buffer
}

/// The palette picture of an original `SimpleImage` at 1:1 (the app's `blit`):
/// the window palette of `FRAMEPIC` and a skipped transparent index.
fn palette_image(source: &ChromeSource, image: &SimpleImage) -> (RgbaBuffer, u32) {
    let mut buffer = RgbaBuffer::new(image.width as u32, image.height as u32);
    let mut painted = 0;

    for y in 0..image.height as u32 {
        for x in 0..image.width as u32 {
            let Some(index) = image.pixel(x, y) else {
                continue;
            };

            if index == image.transparent {
                continue;
            }

            if buffer.set(
                x as i32,
                y as i32,
                Rgba::from_rgb(source.frame().color(index)),
            ) {
                painted += 1;
            }
        }
    }

    (buffer, painted)
}

#[test]
fn the_palette_fallback_of_the_frame_is_the_exact_1to1_picture() {
    let source = original_chrome();
    let reference = palette_frame(&source);

    let chrome = HdChrome::empty(RenderScale::PixelPerfect);
    assert!(!chrome.is_hd());
    assert_eq!(chrome.draw_frame(&mut RgbaBuffer::new(640, 480)), None);

    let mut dst = RgbaBuffer::new(640, 480);
    assert_eq!(
        chrome.draw_frame_with_fallback(&mut dst, &source),
        640 * 480
    );
    assert_eq!(
        dst, reference,
        "S = 1: the fallback must be the original palette FRAMEPIC, byte for byte"
    );
}

#[test]
fn the_palette_fallback_at_hd_is_the_nearest_s_upscale() {
    let source = original_chrome();
    let reference = palette_frame(&source);

    for scale in [RenderScale::Hd2, RenderScale::Hd4] {
        let (width, height) = scale.render_size();
        let chrome = HdChrome::empty(scale);
        let mut dst = RgbaBuffer::new(width, height);

        assert_eq!(
            chrome.draw_frame_with_fallback(&mut dst, &source),
            width * height,
            "{scale:?}"
        );
        assert_eq!(
            dst,
            upscale_nearest(&reference, width, height),
            "{scale:?}: a frame without HD art is the ×S upscale of the palette image"
        );
    }
}

#[test]
fn the_hd_frame_wins_over_the_palette_fallback_at_hd() {
    let cache = cache_with(&[(&chrome_image_tag("FRAMEPIC", 2), "terrain_tile_ETC1S.ktx2")]);
    let chrome = HdChrome::load(&cache, RenderScale::Hd2);
    let source = original_chrome();
    let reference = palette_frame(&source);
    let (width, height) = RenderScale::Hd2.render_size();

    let mut hd = RgbaBuffer::new(width, height);
    assert_eq!(
        chrome.draw_frame_with_fallback(&mut hd, &source),
        width * height
    );
    assert_eq!(
        hd,
        reference_equal_block(
            chrome.frame().expect("HD frame"),
            width as i32,
            height as i32
        ),
        "the HD frame is the source, not the palette picture"
    );
    assert_ne!(
        hd,
        upscale_nearest(&reference, width, height),
        "an existing HD asset must replace the fallback"
    );

    // The same scale without the asset: the palette picture is upscaled.
    let mut fallback = RgbaBuffer::new(width, height);
    assert_eq!(
        HdChrome::empty(RenderScale::Hd2).draw_frame_with_fallback(&mut fallback, &source),
        width * height
    );
    assert_eq!(fallback, upscale_nearest(&reference, width, height));
}

#[test]
fn the_palette_panel_fallback_uses_the_window_palette_and_skips_transparency() {
    let source = original_chrome();
    let panel = source.panel(ChromePanel::Top).expect("PANELTOP");
    let (picture, painted_1to1) = palette_image(&source, panel);
    // 171×239 at S = 2 -> every palette texel becomes a 2×2 block.
    let expected = upscale_nearest(&picture, 342, 478);

    let mut dst = RgbaBuffer::new(342, 478);
    let painted = HdChrome::empty(RenderScale::Hd2).draw_panel_with_fallback(
        &mut dst,
        ChromePanel::Top,
        &source,
    );

    assert_eq!(painted, painted_1to1 * 4);
    assert_eq!(dst, expected);
    assert!(
        painted < 342 * 478,
        "the transparent index of PANELTOP must be skipped"
    );
}

#[test]
fn the_palette_button_fallback_and_its_hd_substitution() {
    let source = original_chrome();
    let button = source.image("FIND_OFF").expect("FIND_OFF");
    let (picture, expected_painted) = palette_image(&source, button);
    let width = button.width as i32;
    let height = button.height as i32;
    let rect = Rect::new(0, 0, width, height);
    let scale = RenderScale::PixelPerfect;

    let mut dst = RgbaBuffer::new(width as u32, height as u32);
    assert_eq!(
        HdChrome::empty(scale).draw_image_with_fallback(&mut dst, "FIND_OFF", rect, &source),
        expected_painted
    );
    assert_eq!(dst, picture, "S = 1 is the original button picture");

    // An HD asset for the same name replaces the palette picture at S > 1.
    let hd_image = HdImage::from_rgba(1, 1, vec![1, 2, 3, 255]).expect("solid HD");
    let hd = HdChrome::with_images(
        RenderScale::Hd2,
        None,
        None,
        None,
        [("FIND_OFF".to_string(), hd_image)].into_iter().collect(),
    );
    let mut hd_dst = RgbaBuffer::new(width as u32 * 2, height as u32 * 2);
    assert_eq!(
        hd.draw_image_with_fallback(&mut hd_dst, "FIND_OFF", rect, &source),
        width as u32 * 2 * height as u32 * 2
    );
    assert_eq!(hd_dst.get(0, 0), Some(Rgba::rgb(1, 2, 3)));
    assert_ne!(
        hd_dst,
        upscale_nearest(&picture, width as u32 * 2, height as u32 * 2)
    );
}
