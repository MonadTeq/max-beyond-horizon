use super::*;

#[test]
fn main_map_matches_original_seven_by_seven_tiles() {
    assert_eq!(MAIN_MAP.ulx, 180);
    assert_eq!(MAIN_MAP.uly, 18);
    assert_eq!(MAIN_MAP.width, 448);
    assert_eq!(MAIN_MAP.height, 448);
    assert!(MAIN_MAP.lrx() < WINDOW_WIDTH, "карта не вылезает за окно");
    assert!(MAIN_MAP.lry() < WINDOW_HEIGHT);
}

#[test]
fn side_panel_is_split_at_239() {
    assert_eq!(SIDE_PANEL_WIDTH, 171);
    assert_eq!(
        SIDE_PANEL_TOP_HEIGHT + (WINDOW_HEIGHT - SIDE_PANEL_TOP_HEIGHT),
        WINDOW_HEIGHT
    );
    assert!(
        SIDE_PANEL_WIDTH < MAIN_MAP.ulx,
            "между панелью и картой есть зазор (скроллбар), как в оригинале"
    );
}

/// Intersection of rectangles (for layout checks).
fn overlaps(a: PixelRect, b: PixelRect) -> bool {
    a.ulx < b.lrx() && a.lrx() > b.ulx && a.uly < b.lry() && a.lry() > b.uly
}

#[test]
fn side_panel_controls_do_not_overlap_main_map() {
    // In the original End Turn deliberately overlaps the map slightly (y=2..23 at
    // uly=18), so it is excluded: it is drawn on top.
    for rect in [
        FILES_BUTTON,
        PREFS_BUTTON,
        SURVEY_BUTTON,
        STATUS_BUTTON,
        COLORS_BUTTON,
        HITS_BUTTON,
        AMMO_BUTTON,
        RANGE_BUTTON,
        SCAN_BUTTON,
        GRID_BUTTON,
        NAME_BUTTON,
        STAT_WINDOW,
        CORNER_FLIC,
        MINIMAP,
    ] {
        assert!(
            !overlaps(rect, MAIN_MAP),
                "элемент {rect:?} пересекается с областью карты"
        );
    }

    assert!(
        overlaps(END_TURN_BUTTON, MAIN_MAP),
            "End Turn в оригинале заходит на карту — если это изменится, поправить раскладку"
    );
}
