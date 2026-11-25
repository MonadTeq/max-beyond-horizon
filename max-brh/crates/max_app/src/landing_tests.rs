use super::*;

/// Map: all land except a vertical water strip in columns 50..52.
fn surface() -> SurfaceMap {
    let mut surface = SurfaceMap::new();

    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            let tile = if (50..53).contains(&x) {
                Surface::WATER
            } else {
                Surface::LAND
            };
            *surface
                .tiles_mut()
                .get_mut(y as usize * MAP_WIDTH as usize + x as usize)
                .unwrap() = tile;
        }
    }

    surface
}

#[test]
fn landing_accepts_only_4x4_land() {
    let mut landing = LandingScreen::new(TeamId::RED, 0, surface());

    landing.cursor = Point::new(20, 20);
    assert!(landing.cursor_valid());
    assert_eq!(landing.confirm(), LandingAction::Confirm);

    landing.cursor = Point::new(51, 20);
    assert!(!landing.cursor_valid());
    assert_eq!(landing.confirm(), LandingAction::None);
    assert!(landing.status.contains("Invalid"));
}

#[test]
fn clicks_map_frame_coordinates_to_grid() {
    assert_eq!(
        LandingScreen::screen_to_grid(LANDING_AREA.ulx, LANDING_AREA.uly),
        Some(Point::new(0, 0))
    );
    assert_eq!(
        LandingScreen::screen_to_grid(LANDING_AREA.ulx + 4 * 10, LANDING_AREA.uly + 4 * 20),
        Some(Point::new(10, 20))
    );
    assert_eq!(LandingScreen::screen_to_grid(0, 0), None, "вне карты");
}

#[test]
fn valid_click_confirms_and_invalid_click_only_moves() {
    let mut landing = LandingScreen::new(TeamId::RED, 0, surface());

    let x = LANDING_AREA.ulx + 30 * LANDING_CELL + 2;
    let y = LANDING_AREA.uly + 22 * LANDING_CELL + 2;

    // A valid cell — C++ confirms the choice immediately.
    assert_eq!(landing.click(x, y), LandingAction::Confirm);
    assert_eq!(landing.cursor, Point::new(30, 22));

    // An invalid one — only moves the point.
    let water_x = LANDING_AREA.ulx + 51 * LANDING_CELL + 2;
    assert_eq!(landing.click(water_x, y), LandingAction::None);
    assert_eq!(landing.cursor, Point::new(51, 22));
    assert!(landing.status.contains("Invalid"));

    // Outside the map area — ignore.
    assert_eq!(landing.click(0, 0), LandingAction::None);
    assert_eq!(landing.cursor, Point::new(51, 22));
}

#[test]
fn arrows_move_the_cursor_within_the_map() {
    let mut landing = LandingScreen::new(TeamId::RED, 0, surface());

    landing.cursor = Point::new(5, 5);
    landing.step(1, 0);
    assert_eq!(landing.cursor, Point::new(6, 5));

    landing.step(-100, -100);
    assert_eq!(landing.cursor, Point::new(1, 1), "кламп по краю карты");
}

#[test]
fn default_cursor_is_a_valid_site() {
    let landing = LandingScreen::new(TeamId::BLUE, 0, surface());

    assert!(
        landing.cursor_valid(),
            "find_valid_starting_position обязан дать валидную точку"
    );
}

#[test]
fn cursor_id_matches_the_cpp_validity_check() {
    let landing = LandingScreen::new(TeamId::RED, 0, surface());
    let center = |grid: Point| {
        (
            LANDING_AREA.ulx + i32::from(grid.x) * LANDING_CELL + LANDING_CELL / 2,
            LANDING_AREA.uly + i32::from(grid.y) * LANDING_CELL + LANDING_CELL / 2,
        )
    };

    let (x, y) = center(Point::new(30, 22));
    assert_eq!(
        cursor_for_screen_point(&landing, x, y),
        Some(crate::cursor::CursorId::UnitGo),
            "CURSOR_UNIT_GO над валидной клеткой"
    );

    let (x, y) = center(Point::new(51, 22));
    assert_eq!(
        cursor_for_screen_point(&landing, x, y),
        Some(crate::cursor::CursorId::UnitNoGo),
            "CURSOR_UNIT_NO_GO над невалидной клеткой"
    );

    assert_eq!(
        cursor_for_screen_point(&landing, 0, 0),
        None,
            "вне карты — HANDPTR (SDL-слой)"
    );
}

#[test]
fn message_box_is_one_line_and_darkens_the_map() {
    let landing = LandingScreen::new(TeamId::RED, 0, surface());
    let mut colors = [[0u8, 0, 0]; 256];
    colors[usize::from(MESSAGE_TEXT_COLOR)] = [255, 255, 255];
    colors[1] = [52, 60, 72];
    let palette = crate::panels::UiPalette::from_colors(colors);
    let water = Rgba::rgb(80, 128, 164);
    let mut frame = RgbaBuffer::filled(640, 480, water);

    let painted = landing.draw_message(&mut frame, &palette);

    assert!(painted > 0);
    // Exactly one line high: `20 + 10` (`MessageManager_MessageBox_Height`).
    let rect = PixelRect::new(
        LANDING_AREA.ulx,
        LANDING_AREA.uly + MESSAGE_TOP_OFFSET,
        GnwFontId::Font5.font().text_width(MESSAGE_TEXT) + MESSAGE_TEXT_LEFT * 2,
        MESSAGE_BASE_HEIGHT + MESSAGE_LINE_HEIGHT,
    );
    assert_ne!(
        frame.get(rect.ulx, rect.uly),
        Some(water),
            "фон окна затемнён по палитре"
    );
    assert_eq!(
        frame.get(rect.ulx - 1, rect.uly),
        Some(water),
            "за окном карта не тронута"
    );
}
