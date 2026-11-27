use super::*;

/// A `width × height` image whose texel `(x, y)` encodes its coordinates:
/// `R = x`, `G = y`, `B = x + y`, `A = 255`.
fn coordinate_image(width: u32, height: u32) -> HdImage {
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);

    for y in 0..height {
        for x in 0..width {
            rgba.extend_from_slice(&[x as u8, y as u8, (x + y) as u8, 255]);
        }
    }

    HdImage::from_rgba(width, height, rgba).expect("coordinate image")
}

/// A `width × height` image of one opaque color.
fn solid_image(width: u32, height: u32, color: Rgba) -> HdImage {
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);

    for _ in 0..width * height {
        rgba.extend_from_slice(&color.to_rgba());
    }

    HdImage::from_rgba(width, height, rgba).expect("solid image")
}

#[test]
fn hd_image_rejects_degenerate_textures() {
    assert!(HdImage::from_rgba(0, 0, Vec::new()).is_none(), "empty");
    assert!(
        HdImage::from_rgba(0, 5, vec![0; 5 * 4]).is_none(),
        "no width"
    );
    assert!(
        HdImage::from_rgba(2, 3, vec![0; 2 * 3 * 4 - 1]).is_none(),
        "truncated RGBA"
    );
    assert!(
        HdImage::from_rgba(2, 3, vec![0; 2 * 3 * 4 + 4]).is_none(),
        "trailing bytes"
    );
    assert!(HdImage::from_rgba(2, 3, vec![0; 2 * 3 * 4]).is_some());
}

#[test]
fn hd_image_reads_texels_with_alpha() {
    let image = HdImage::from_rgba(
        2,
        2,
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
    )
    .unwrap();

    assert!(!image.is_empty());
    assert_eq!(image.width(), 2);
    assert_eq!(image.height(), 2);
    assert_eq!(image.pixels().len(), 16);
    assert_eq!(image.texel(0, 0), Some(Rgba::from_rgba([1, 2, 3, 4])));
    assert_eq!(image.texel(1, 1), Some(Rgba::from_rgba([13, 14, 15, 16])));
    assert_eq!(image.texel(2, 0), None);
    assert_eq!(image.texel(0, 2), None);
}

#[test]
fn stretch_into_uses_the_equal_block_mapping() {
    let image = coordinate_image(4, 4);
    let mut dst = RgbaBuffer::new(8, 8);

    assert_eq!(
        stretch_into(&mut dst, &image, PixelRect::new(0, 0, 8, 8)),
        64,
        "every destination pixel is painted"
    );

    // local * 4 / 8: 0,0,1,1,2,2,3,3 on both axes.
    for y in 0..8usize {
        for x in 0..8usize {
            let expected = Rgba::rgb((x / 2) as u8, (y / 2) as u8, (x / 2 + y / 2) as u8);
            assert_eq!(dst.get(x as i32, y as i32), Some(expected), "({x}, {y})");
        }
    }
}

#[test]
fn stretch_into_maps_non_square_destinations() {
    let image = coordinate_image(4, 2);
    let mut dst = RgbaBuffer::new(16, 8);

    // columns: x * 4 / 16 -> x / 4; rows: y * 2 / 8 -> y / 4.
    assert_eq!(
        stretch_into(&mut dst, &image, PixelRect::new(0, 0, 16, 8)),
        16 * 8
    );

    assert_eq!(dst.get(0, 0), Some(Rgba::rgb(0, 0, 0)));
    assert_eq!(dst.get(3, 3), Some(Rgba::rgb(0, 0, 0)));
    assert_eq!(dst.get(4, 0), Some(Rgba::rgb(1, 0, 1)));
    assert_eq!(dst.get(15, 7), Some(Rgba::rgb(3, 1, 4)));
}

#[test]
fn stretch_into_clips_to_the_destination_and_counts_written_pixels() {
    let image = solid_image(2, 2, Rgba::rgb(10, 20, 30));
    let mut dst = RgbaBuffer::new(4, 4);

    // The rectangle starts at (2, 2); only the visible 2×2 corner is written.
    let painted = stretch_into(&mut dst, &image, PixelRect::new(2, 2, 4, 4));

    assert_eq!(painted, 4);
    assert_eq!(dst.get(2, 2), Some(Rgba::rgb(10, 20, 30)));
    assert_eq!(dst.get(3, 3), Some(Rgba::rgb(10, 20, 30)));
    assert_eq!(dst.get(0, 0), Some(Rgba::TRANSPARENT), "nothing above-left");
}

#[test]
fn stretch_into_skips_fully_transparent_texels() {
    // Left column transparent, right column opaque.
    let image = HdImage::from_rgba(2, 1, vec![9, 9, 9, 0, 7, 8, 9, 255]).unwrap();
    let mut dst = RgbaBuffer::filled(2, 1, Rgba::rgb(1, 2, 3));

    assert_eq!(
        stretch_into(&mut dst, &image, PixelRect::new(0, 0, 2, 1)),
        1,
        "the transparent texel is not written"
    );
    assert_eq!(dst.get(0, 0), Some(Rgba::rgb(1, 2, 3)), "cut-out kept");
    assert_eq!(dst.get(1, 0), Some(Rgba::rgb(7, 8, 9)));
}

#[test]
fn an_empty_rectangle_paints_nothing() {
    let image = solid_image(2, 2, Rgba::WHITE);
    let mut dst = RgbaBuffer::new(4, 4);

    assert_eq!(
        stretch_into(&mut dst, &image, PixelRect::new(0, 0, 0, 4)),
        0
    );
    assert_eq!(dst, RgbaBuffer::new(4, 4));
}

#[test]
fn stretch_sampled_uses_the_equal_block_mapping_and_skips_missing_texels() {
    // Source 2x2: the left column is "transparent" (None), the right one is a
    // coordinate color. The generic sampler backs the palette fallback.
    let sample = |x: u32, y: u32| {
        if x == 0 {
            None
        } else {
            Some(Rgba::rgb(x as u8, y as u8, (x + y) as u8))
        }
    };
    let mut dst = RgbaBuffer::filled(4, 4, Rgba::rgb(9, 9, 9));

    // local * 2 / 4: columns 0,0,1,1; rows 0,0,1,1.
    assert_eq!(
        stretch_sampled(&mut dst, PixelRect::new(0, 0, 4, 4), 2, 2, sample),
        8,
        "two opaque source columns x two rows x four destination rows"
    );
    assert_eq!(
        dst.get(0, 0),
        Some(Rgba::rgb(9, 9, 9)),
        "None keeps the destination"
    );
    assert_eq!(
        dst.get(1, 3),
        Some(Rgba::rgb(9, 9, 9)),
        "None keeps the destination"
    );
    assert_eq!(dst.get(2, 0), Some(Rgba::rgb(1, 0, 1)));
    assert_eq!(dst.get(3, 1), Some(Rgba::rgb(1, 0, 1)));
    assert_eq!(dst.get(2, 2), Some(Rgba::rgb(1, 1, 2)));
    assert_eq!(dst.get(3, 3), Some(Rgba::rgb(1, 1, 2)));
}

#[test]
fn scaled_rect_is_the_identity_at_pixel_perfect_and_times_s_in_hd() {
    let rect = Rect::new(180, 18, 448, 448);

    assert_eq!(
        scaled_rect(rect, RenderScale::PixelPerfect),
        PixelRect::new(180, 18, 448, 448)
    );
    assert_eq!(
        scaled_rect(rect, RenderScale::Hd2),
        PixelRect::new(360, 36, 896, 896)
    );
    assert_eq!(
        scaled_rect(rect, RenderScale::Hd4),
        PixelRect::new(720, 72, 1792, 1792)
    );

    // The `layout` constants are in the same 640×480 space.
    assert_eq!(
        scaled_pixel_rect(layout::MAIN_MAP, RenderScale::Hd2),
        PixelRect::new(360, 36, 896, 896)
    );
    assert_eq!(
        scaled_pixel_rect(layout::MAIN_MAP, RenderScale::PixelPerfect),
        layout::MAIN_MAP
    );
}

#[test]
fn panel_halves_cover_the_left_side_of_the_window() {
    // 171×239 + 171×241 = the whole 480 px height (R0 §2: the two resource
    // halves are 239 and 241 px).
    assert_eq!(PANEL_TOP_RECT, Rect::new(0, 0, 171, 239));
    assert_eq!(PANEL_BOTTOM_RECT, Rect::new(0, 239, 171, 241));
    assert_eq!(
        ChromePanel::Top.logical_rect().bottom(),
        ChromePanel::Bottom.logical_rect().y,
        "the halves meet without a gap"
    );
    assert_eq!(
        ChromePanel::Bottom.logical_rect().bottom(),
        layout::WINDOW_HEIGHT,
        "the lower half ends at the window bottom"
    );
    assert_eq!(ChromePanel::Top.resource(), "PANELTOP");
    assert_eq!(ChromePanel::Bottom.resource(), "PANELBTM");
    assert_eq!(ChromePanel::ALL.len(), 2);
}

#[test]
fn an_empty_chrome_falls_back_on_every_asset() {
    for scale in [RenderScale::Hd2, RenderScale::Hd4] {
        let chrome = HdChrome::empty(scale);
        let mut dst = RgbaBuffer::new(64, 48);

        assert!(chrome.is_hd());
        assert_eq!(chrome.hd_count(), 0);
        assert!(chrome.frame().is_none());
        assert!(chrome.panel_top().is_none());
        assert!(chrome.panel_bottom().is_none());
        assert!(chrome.image("ENDTRN_U").is_none());
        assert_eq!(chrome.draw_frame(&mut dst), None);
        assert_eq!(chrome.draw_panel(&mut dst, ChromePanel::Top), None);
        assert_eq!(chrome.draw_panel(&mut dst, ChromePanel::Bottom), None);
        assert_eq!(
            chrome.draw_image(&mut dst, "ENDTRN_U", Rect::new(0, 0, 10, 10)),
            None
        );
        assert_eq!(dst, RgbaBuffer::new(64, 48), "nothing is drawn");
    }
}

#[test]
fn pixel_perfect_never_substitutes_hd_assets() {
    // Even an explicitly injected HD texture must not be used at S = 1: the
    // pixel-perfect path is byte-for-byte and owns the golden frames.
    let image = solid_image(2, 2, Rgba::rgb(10, 20, 30));
    let chrome = HdChrome::with_images(
        RenderScale::PixelPerfect,
        Some(image.clone()),
        Some(image.clone()),
        Some(image.clone()),
        [("ENDTRN_U".to_string(), image.clone())]
            .into_iter()
            .collect(),
    );
    let mut dst = RgbaBuffer::new(16, 12);

    assert!(!chrome.is_hd());
    assert_eq!(chrome.draw_frame(&mut dst), None);
    assert_eq!(chrome.draw_panel(&mut dst, ChromePanel::Top), None);
    assert_eq!(
        chrome.draw_image(&mut dst, "ENDTRN_U", Rect::new(0, 0, 8, 8)),
        None
    );
    assert_eq!(dst, RgbaBuffer::new(16, 12));

    let skin = HdUiSkin::with_parts(RenderScale::PixelPerfect, [(UiBox::Button, image)]);
    let mut target = RgbaBuffer::new(8, 8);
    assert!(
        !skin.paint_box(&mut target, Rect::new(0, 0, 8, 8), UiBox::Button),
        "a PixelPerfect skin never paints"
    );
    assert_eq!(target, RgbaBuffer::new(8, 8));
}

#[test]
fn draw_frame_maps_the_hd_texture_onto_the_render_frame() {
    let image = coordinate_image(2, 2);
    let chrome = HdChrome::with_images(
        RenderScale::Hd2,
        Some(image.clone()),
        None,
        None,
        BTreeMap::new(),
    );
    let mut dst = RgbaBuffer::new(1280, 960);

    assert_eq!(chrome.frame_rect(), PixelRect::new(0, 0, 1280, 960));
    assert_eq!(chrome.hd_count(), 1);
    assert_eq!(chrome.draw_frame(&mut dst), Some(1280 * 960));

    // A 2×2 source over a 1280×960 frame: each texel is a 640×480 block.
    assert_eq!(dst.get(0, 0), image.texel(0, 0));
    assert_eq!(dst.get(639, 479), image.texel(0, 0));
    assert_eq!(dst.get(640, 0), image.texel(1, 0));
    assert_eq!(dst.get(0, 480), image.texel(0, 1));
    assert_eq!(dst.get(1279, 959), image.texel(1, 1));
}

#[test]
fn draw_panel_places_the_halves_on_the_left_panel() {
    let top = solid_image(1, 1, Rgba::rgb(1, 2, 3));
    let bottom = solid_image(1, 1, Rgba::rgb(4, 5, 6));
    let chrome = HdChrome::with_images(
        RenderScale::Hd2,
        None,
        Some(top),
        Some(bottom),
        BTreeMap::new(),
    );
    let mut dst = RgbaBuffer::new(342, 960);

    // 171×239 at S = 2 -> 342×478 painted pixels.
    assert_eq!(
        chrome.draw_panel(&mut dst, ChromePanel::Top),
        Some(342 * 478)
    );
    assert_eq!(dst.get(0, 0), Some(Rgba::rgb(1, 2, 3)));
    assert_eq!(dst.get(341, 477), Some(Rgba::rgb(1, 2, 3)));
    assert_eq!(
        dst.get(0, 478),
        Some(Rgba::TRANSPARENT),
        "the bottom half was not drawn yet"
    );

    // 171×241 at S = 2 -> 342×482 painted pixels, starting at y = 239·2.
    assert_eq!(
        chrome.draw_panel(&mut dst, ChromePanel::Bottom),
        Some(342 * 482)
    );
    assert_eq!(dst.get(0, 478), Some(Rgba::rgb(4, 5, 6)));
    assert_eq!(dst.get(341, 959), Some(Rgba::rgb(4, 5, 6)));
}

#[test]
fn draw_image_draws_named_assets_and_missing_names_fall_back() {
    let button = solid_image(2, 2, Rgba::rgb(7, 8, 9));
    let chrome = HdChrome::with_images(
        RenderScale::Hd2,
        None,
        None,
        None,
        [("ENDTRN_U".to_string(), button)].into_iter().collect(),
    );
    let mut dst = RgbaBuffer::new(144, 42);
    // The End Turn button is 72×21 logically (layout::END_TURN_BUTTON); at S = 2
    // that is a 144×42 destination.
    let rect = Rect::new(0, 0, 72, 21);

    assert_eq!(
        chrome.draw_image(&mut dst, "ENDTRN_U", rect),
        Some(144 * 42)
    );
    assert_eq!(dst.get(0, 0), Some(Rgba::rgb(7, 8, 9)));
    assert_eq!(dst.get(143, 41), Some(Rgba::rgb(7, 8, 9)));
    assert!(chrome.image("ENDTRN_U").is_some());

    // A name the bank does not know is a per-asset fallback, not an error.
    assert_eq!(chrome.draw_image(&mut dst, "MNUBTN", rect), None);
}

#[test]
fn hd_ui_skin_paints_and_falls_back_to_the_button_skin() {
    let button = coordinate_image(2, 2);
    let skin = HdUiSkin::with_parts(RenderScale::Hd2, [(UiBox::Button, button)]);
    let rect = Rect::new(0, 0, 4, 4);
    let mut dst = RgbaBuffer::new(4, 4);

    assert!(skin.is_hd());
    assert!(!skin.is_empty());
    assert_eq!(skin.part_count(), 1);
    assert!(skin.part(UiBox::Button).is_some());
    assert!(skin.part(UiBox::ButtonHover).is_none());

    // The base button skin paints the normal state.
    assert!(skin.paint_box(&mut dst, rect, UiBox::Button));
    assert_eq!(dst.get(0, 0), Some(Rgba::rgb(0, 0, 0)));
    assert_eq!(dst.get(1, 1), Some(Rgba::rgb(0, 0, 0)));
    assert_eq!(dst.get(2, 2), Some(Rgba::rgb(1, 1, 2)));
    assert_eq!(dst.get(3, 3), Some(Rgba::rgb(1, 1, 2)));

    // Hover/disabled fall back to the same base skin, so a single button
    // texture keeps every button state HD.
    let mut hover = RgbaBuffer::new(4, 4);
    assert!(skin.paint_box(&mut hover, rect, UiBox::ButtonHover));
    assert_eq!(hover, dst);
    let mut disabled = RgbaBuffer::new(4, 4);
    assert!(skin.paint_box(&mut disabled, rect, UiBox::ButtonDisabled));
    assert_eq!(disabled, dst);

    // No panel skin: the caller keeps the procedural theme drawing.
    assert!(!skin.paint_box(&mut dst, rect, UiBox::Panel));
}

#[test]
fn hd_ui_skin_without_assets_is_a_full_fallback() {
    let skin = HdUiSkin::empty(RenderScale::Hd4);

    assert!(skin.is_empty());
    assert_eq!(skin.part_count(), 0);
    assert!(!skin.paint_box(
        &mut RgbaBuffer::new(4, 4),
        Rect::new(0, 0, 4, 4),
        UiBox::Background
    ));
}

#[test]
fn load_without_a_manifest_is_empty_for_chrome_and_skins() {
    let cache = RemasterCache::open("__no_such_remaster_root__");

    for scale in [RenderScale::Hd2, RenderScale::Hd4] {
        let chrome = HdChrome::load_with(&cache, scale, &["ENDTRN_U", "MNUBTN"]);
        assert_eq!(chrome.hd_count(), 0);
        assert_eq!(chrome.scale(), scale);

        let skin = HdUiSkin::load(&cache, scale);
        assert!(skin.is_empty());
        assert_eq!(skin.scale(), scale);
    }

    // S = 1 never even asks the loader.
    assert_eq!(
        HdChrome::load(&cache, RenderScale::PixelPerfect).hd_count(),
        0
    );
    assert!(HdUiSkin::load(&cache, RenderScale::PixelPerfect).is_empty());
}

#[test]
fn empty_and_with_images_constructors_report_their_assets() {
    let image = solid_image(1, 1, Rgba::WHITE);
    let chrome = HdChrome::with_images(
        RenderScale::Hd4,
        Some(image.clone()),
        Some(image.clone()),
        None,
        [("A".to_string(), image.clone())].into_iter().collect(),
    );

    assert_eq!(chrome.hd_count(), 3);
    assert_eq!(chrome.frame_rect(), PixelRect::new(0, 0, 2560, 1920));
    assert_eq!(
        chrome.render_rect(Rect::new(10, 20, 30, 40)),
        PixelRect::new(40, 80, 120, 160)
    );
}
