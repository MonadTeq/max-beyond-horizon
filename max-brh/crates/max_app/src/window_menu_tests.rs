use super::*;

use max_render::buffer::PixelRect;

fn center(rect: PixelRect) -> (i32, i32) {
    (rect.ulx + rect.width / 2, rect.uly + rect.height / 2)
}

fn click(menu: &mut WindowMenu, x: i32, y: i32) -> MenuResponse {
    menu.handle_input(InputEvent::MouseClick(x, y))
}

/// Opens `Settings → Resolutions` with the pointer, without picking a row.
fn open_resolutions(menu: &mut WindowMenu) {
    let (x, y) = center(title_rect(MenuPage::Settings));
    click(menu, x, y);
    let (x, y) = center(item_rect(MenuItem::Resolutions));
    click(menu, x, y);
}

fn count_color(buffer: &RgbaBuffer, rect: PixelRect, color: Rgba) -> usize {
    let mut count = 0;

    for y in rect.uly..rect.lry() {
        for x in rect.ulx..rect.lrx() {
            if buffer.get(x, y) == Some(color) {
                count += 1;
            }
        }
    }

    count
}

#[test]
fn a_closed_bar_eats_only_the_bar_and_passes_the_frame_through() {
    let mut menu = WindowMenu::new();

    // With no dropdown open the frame below the bar belongs to the game.
    let response = menu.handle_input(InputEvent::MouseClick(320, 240));
    assert!(!response.consumed);
    assert_eq!(response.action, WindowMenuAction::None);

    // The bar itself is window chrome: the click opens File and is swallowed.
    let (x, y) = center(title_rect(MenuPage::File));
    let response = click(&mut menu, x, y);

    assert!(response.consumed);
    assert_eq!(response.action, WindowMenuAction::None);
    assert_eq!(menu.open_page(), Some(MenuPage::File));
}

#[test]
fn file_has_the_single_exit_item() {
    let mut menu = WindowMenu::new();
    let (x, y) = center(title_rect(MenuPage::File));
    click(&mut menu, x, y);

    let (x, y) = center(item_rect(MenuItem::Exit));
    let response = click(&mut menu, x, y);

    assert_eq!(response.action, WindowMenuAction::Quit);
    assert!(response.consumed);
    assert!(!menu.is_open(), "Exit closes the menu as well");
}

#[test]
fn clicking_the_open_title_closes_the_dropdown() {
    let mut menu = WindowMenu::new();
    let (x, y) = center(title_rect(MenuPage::File));

    click(&mut menu, x, y);
    assert!(menu.is_open());

    let response = click(&mut menu, x, y);
    assert!(response.consumed);
    assert!(!menu.is_open());
}

#[test]
fn settings_resolutions_apply_every_shipped_scale() {
    for scale in RESOLUTIONS {
        let mut menu = WindowMenu::new();
        open_resolutions(&mut menu);

        assert!(menu.is_flyout_open(), "the flyout is open for {scale:?}");

        let (x, y) = center(item_rect(MenuItem::Resolution(scale)));
        let response = click(&mut menu, x, y);

        assert_eq!(
            response.action,
            WindowMenuAction::SetResolution(scale),
            "row {scale:?} must select exactly that render scale"
        );
        assert!(response.consumed);
        assert!(!menu.is_open());
        assert!(!menu.is_flyout_open());
    }
}

#[test]
fn clicking_outside_the_open_menu_dismisses_it() {
    let mut menu = WindowMenu::new();
    open_resolutions(&mut menu);

    let response = menu.handle_input(InputEvent::MouseClick(320, 300));

    assert!(response.consumed, "a click outside dismisses the menu");
    assert!(!menu.is_open());
}

#[test]
fn hovering_the_resolutions_row_opens_the_flyout() {
    let mut menu = WindowMenu::new();
    let (x, y) = center(title_rect(MenuPage::Settings));
    click(&mut menu, x, y);

    let (x, y) = center(item_rect(MenuItem::Resolutions));
    let response = menu.handle_input(InputEvent::MouseMove(x, y));

    assert!(!response.consumed, "mouse moves still reach the game");
    assert!(menu.is_flyout_open());

    let (x, y) = center(item_rect(MenuItem::Resolution(RenderScale::Hd4)));
    menu.handle_input(InputEvent::MouseMove(x, y));

    assert!(
        menu.is_flyout_open(),
        "the flyout follows the pointer into it"
    );

    // Moving back to the frame folds the flyout away.
    menu.handle_input(InputEvent::MouseMove(320, 300));
    assert!(!menu.is_flyout_open());
}

#[test]
fn escape_closes_an_open_menu_and_is_otherwise_passed_through() {
    let mut menu = WindowMenu::new();
    let (x, y) = center(title_rect(MenuPage::Settings));
    click(&mut menu, x, y);

    let response = menu.handle_input(InputEvent::Key(Key::Escape));
    assert!(response.consumed);
    assert!(!menu.is_open());

    let response = menu.handle_input(InputEvent::Key(Key::Escape));
    assert!(!response.consumed, "a closed menu leaves Esc to the screen");
}

#[test]
fn right_click_dismisses_the_menu() {
    let mut menu = WindowMenu::new();
    let (x, y) = center(title_rect(MenuPage::File));
    click(&mut menu, x, y);

    let response = menu.handle_input(InputEvent::RightClick(320, 240));

    assert!(response.consumed);
    assert!(!menu.is_open());
}

#[test]
fn labels_name_the_three_resolutions_as_the_requirement_does() {
    assert_eq!(resolution_label(RenderScale::PixelPerfect), "640x480 (1:1)");
    assert_eq!(resolution_label(RenderScale::Hd2), "1280x960 (x2)");
    assert_eq!(resolution_label(RenderScale::Hd4), "2560x1920 (x4)");
}

#[test]
fn the_flyout_fits_the_logical_frame() {
    for parent in [MenuItem::Resolutions, MenuItem::Antialiasing] {
        let flyout = flyout_rect(parent);

        assert!(flyout.ulx > 0);
        assert!(
            flyout.lrx() <= 640,
            "{parent:?}: flyout right edge {} exceeds 640",
            flyout.lrx()
        );
        assert!(
            flyout.lry() <= 480,
            "{parent:?}: flyout bottom edge {} exceeds 480",
            flyout.lry()
        );
    }
}

#[test]
fn the_resolution_rows_are_stacked() {
    // Only one dropdown is open at a time, so the absolute rects of File and
    // Settings may overlap; the flyout rows themselves must not.
    let rows: Vec<PixelRect> = RESOLUTIONS
        .into_iter()
        .map(|scale| item_rect(MenuItem::Resolution(scale)))
        .collect();

    for pair in rows.windows(2) {
        assert!(
            pair[0].lry() <= pair[1].uly,
            "resolution rows must be stacked, got {:?} and {:?}",
            pair[0],
            pair[1]
        );
    }
}

#[test]
fn the_settings_dropdown_has_the_two_flyout_rows() {
    let resolutions = item_rect(MenuItem::Resolutions);
    let antialiasing = item_rect(MenuItem::Antialiasing);

    assert!(
        resolutions.lry() <= antialiasing.uly,
        "the Settings rows overlap"
    );
    assert_eq!(
        dropdown_rect(MenuPage::Settings).height,
        2 * ROW_HEIGHT + 2,
        "the dropdown grows with the second row"
    );
    // Both flyouts open next to the Settings dropdown, under their own row.
    for parent in MenuItem::PARENTS {
        let flyout = flyout_rect(parent);

        assert_eq!(flyout.ulx, dropdown_rect(MenuPage::Settings).lrx());
        assert_eq!(flyout.uly, item_rect(parent).uly, "{parent:?}");
    }
}

#[test]
fn the_overlay_is_transparent_outside_the_menu() {
    let mut menu = WindowMenu::new();
    open_resolutions(&mut menu);

    let mut overlay = RgbaBuffer::new(640, 480);
    menu.render_overlay(&mut overlay, RenderScale::Hd2, AaMode::Off);

    // The bar is opaque.
    assert_eq!(overlay.get(500, 5), Some(BAR_BACKGROUND));
    // The game under the dropdowns stays transparent.
    assert!(overlay.get(320, 300).expect("inside").is_transparent());
    // The flyout is opaque and names three rows.
    let flyout = flyout_rect(MenuItem::Resolutions);
    assert_eq!(
        overlay.get(flyout.ulx + 4, flyout.uly + 2),
        Some(MENU_BACKGROUND)
    );
}

#[test]
fn the_current_resolution_is_marked() {
    for current in RESOLUTIONS {
        let mut menu = WindowMenu::new();
        open_resolutions(&mut menu);

        let mut overlay = RgbaBuffer::new(640, 480);
        menu.render_overlay(&mut overlay, current, AaMode::Off);

        let index = RESOLUTIONS
            .into_iter()
            .position(|scale| scale == current)
            .expect("shipped scale");

        for (row, scale) in RESOLUTIONS.into_iter().enumerate() {
            let rect = flyout_row_rect(MenuItem::Resolutions, row);
            let marked = count_color(&overlay, rect, CURRENT_COLOR);

            if row == index {
                assert!(
                    marked > 0,
                    "{scale:?} must be marked as the current resolution"
                );
            } else {
                assert_eq!(
                    marked, 0,
                    "{scale:?} must not be marked while {current:?} is current"
                );
            }
        }
    }
}

#[test]
fn the_bar_is_drawn_near_the_top_edge() {
    let mut overlay = RgbaBuffer::new(640, 480);
    WindowMenu::new().render_overlay(&mut overlay, RenderScale::PixelPerfect, AaMode::Off);

    for x in [0, 200, 639] {
        assert_eq!(
            overlay.get(x, 0),
            Some(BAR_BACKGROUND),
            "the bar must cover the full width at x={x}"
        );
    }

    assert!(overlay
        .get(0, BAR_HEIGHT)
        .expect("below the bar")
        .is_transparent());
}

#[test]
fn window_events_are_translated_into_the_game_frame() {
    let bar = BAR_HEIGHT;

    assert_eq!(
        to_game_event(InputEvent::MouseClick(100, 20)),
        InputEvent::MouseClick(100, 20 - bar)
    );
    assert_eq!(
        to_game_event(InputEvent::MouseRelease(100, 20)),
        InputEvent::MouseRelease(100, 20 - bar)
    );
    assert_eq!(
        to_game_event(InputEvent::RightClick(100, 20)),
        InputEvent::RightClick(100, 20 - bar)
    );
    assert_eq!(
        to_game_event(InputEvent::MouseMove(100, 20)),
        InputEvent::MouseMove(100, 20 - bar)
    );
    // Keys are not tied to the presentation band.
    assert_eq!(
        to_game_event(InputEvent::Key(Key::Escape)),
        InputEvent::Key(Key::Escape)
    );

    // The first game pixel starts directly below the bar.
    assert_eq!(
        to_game_event(InputEvent::MouseClick(0, bar)),
        InputEvent::MouseClick(0, 0)
    );
}

#[test]
fn composite_overlay_scales_by_nearest_neighbour() {
    let mut overlay = RgbaBuffer::new(4, 4);
    overlay.set(1, 1, Rgba::rgb(200, 30, 40));

    let mut target = RgbaBuffer::filled(8, 8, Rgba::BLACK);
    composite_overlay(&overlay, &mut target, 2);

    let red = Rgba::rgb(200, 30, 40);
    for (x, y) in [(2, 2), (3, 2), (2, 3), (3, 3)] {
        assert_eq!(target.get(x, y), Some(red), "pixel ({x},{y})");
    }

    // A neighbour overlay pixel was transparent: the target keeps its color.
    assert_eq!(target.get(0, 0), Some(Rgba::BLACK));
}

#[test]
fn composite_overlay_blends_semi_transparent_pixels() {
    let mut overlay = RgbaBuffer::new(2, 2);
    overlay.set(0, 0, Rgba::from_rgba([255, 255, 255, 128]));

    let mut target = RgbaBuffer::filled(2, 2, Rgba::BLACK);
    composite_overlay(&overlay, &mut target, 1);

    let blended = target.get(0, 0).expect("pixel");

    assert_eq!(blended.a, 255);
    assert!((127..=128).contains(&blended.r), "{blended:?}");
}

#[test]
fn composite_overlay_with_scale_one_writes_exactly() {
    let mut overlay = RgbaBuffer::new(3, 3);
    overlay.set(2, 1, Rgba::rgb(1, 2, 3));

    let mut target = RgbaBuffer::filled(3, 3, Rgba::BLACK);
    composite_overlay(&overlay, &mut target, 1);

    assert_eq!(target.get(2, 1), Some(Rgba::rgb(1, 2, 3)));
    assert_eq!(target.get(0, 0), Some(Rgba::BLACK));
}

// ---------------------------------------------------------------------------
// Settings → Antialiasing (SSAA, ssaa-design.md)
// ---------------------------------------------------------------------------

/// Opens `Settings → Antialiasing` with the pointer, without picking a row.
fn open_antialiasing(menu: &mut WindowMenu) {
    let (x, y) = center(title_rect(MenuPage::Settings));
    click(menu, x, y);
    let (x, y) = center(item_rect(MenuItem::Antialiasing));
    click(menu, x, y);
}

#[test]
fn settings_antialiasing_applies_every_shipped_mode() {
    for aa in AA_MODES {
        let mut menu = WindowMenu::new();
        open_antialiasing(&mut menu);

        assert!(menu.is_flyout_open(), "the flyout is open for {aa:?}");
        assert_eq!(menu.flyout_parent(), Some(MenuItem::Antialiasing));

        let (x, y) = center(item_rect(MenuItem::Aa(aa)));
        let response = click(&mut menu, x, y);

        assert_eq!(
            response.action,
            WindowMenuAction::SetAa(aa),
            "row {aa:?} must select exactly that SSAA mode"
        );
        assert!(response.consumed);
        assert!(!menu.is_open());
        assert!(!menu.is_flyout_open());
    }
}

#[test]
fn the_aa_rows_are_off_2x_4x_and_stacked() {
    assert_eq!(aa_label(AaMode::Off), "Off");
    assert_eq!(aa_label(AaMode::X2), "2x");
    assert_eq!(aa_label(AaMode::X4), "4x");

    let rows: Vec<PixelRect> = AA_MODES
        .into_iter()
        .map(|aa| item_rect(MenuItem::Aa(aa)))
        .collect();

    for pair in rows.windows(2) {
        assert!(pair[0].lry() <= pair[1].uly, "the AA rows overlap");
    }
}

#[test]
fn the_current_antialiasing_mode_is_marked() {
    for current in AA_MODES {
        let mut menu = WindowMenu::new();
        open_antialiasing(&mut menu);

        let mut overlay = RgbaBuffer::new(640, 480);
        menu.render_overlay(&mut overlay, RenderScale::Hd2, current);

        let index = AA_MODES
            .into_iter()
            .position(|aa| aa == current)
            .expect("a shipped mode");

        for (row, aa) in AA_MODES.into_iter().enumerate() {
            let marked = count_color(
                &overlay,
                flyout_row_rect(MenuItem::Antialiasing, row),
                CURRENT_COLOR,
            );

            if row == index {
                assert!(marked > 0, "{aa:?} must be marked as current");
            } else {
                assert_eq!(marked, 0, "{aa:?} must not be marked while {current:?}");
            }
        }
    }
}

/// Hovering `Resolutions` and then `Antialiasing` moves the open flyout instead
/// of leaving both live; the folded-away resolution flyout is not hit-testable.
///
/// The two flyouts share the screen area to the right of the dropdown (only one
/// is drawn at a time), so the test picks the only resolution row that lies
/// above the antialiasing flyout: a click there is outside the open menu and
/// dismisses it without selecting anything.
#[test]
fn only_one_flyout_is_open_at_a_time() {
    let mut menu = WindowMenu::new();
    open_resolutions(&mut menu);
    assert_eq!(menu.flyout_parent(), Some(MenuItem::Resolutions));

    let (x, y) = center(item_rect(MenuItem::Antialiasing));
    menu.handle_input(InputEvent::MouseMove(x, y));

    assert_eq!(menu.flyout_parent(), Some(MenuItem::Antialiasing));

    // The first resolution row is above `flyout_rect(Antialiasing)`: the click
    // lands outside the open menu and dismisses it instead of selecting 1:1.
    let (x, y) = center(item_rect(MenuItem::Resolution(RenderScale::PixelPerfect)));
    let response = click(&mut menu, x, y);

    assert_eq!(response.action, WindowMenuAction::None);
    assert!(!menu.is_flyout_open());
}
