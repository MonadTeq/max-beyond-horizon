use super::*;

#[test]
fn zones_match_the_cpp_window_table() {
    // Side middles and diagonal sectors.
    assert_eq!(zone_at(300, 4), Some(ScrollZone::Up));
    assert_eq!(zone_at(600, 4), Some(ScrollZone::UpRight));
    assert_eq!(zone_at(636, 20), Some(ScrollZone::RightUp));
    assert_eq!(zone_at(636, 200), Some(ScrollZone::Right));
    assert_eq!(zone_at(636, 460), Some(ScrollZone::RightDown));
    assert_eq!(zone_at(600, 476), Some(ScrollZone::DownRight));
    assert_eq!(zone_at(300, 476), Some(ScrollZone::Down));
    assert_eq!(zone_at(20, 476), Some(ScrollZone::DownLeft));
    assert_eq!(zone_at(4, 460), Some(ScrollZone::LeftDown));
    assert_eq!(zone_at(4, 200), Some(ScrollZone::Left));
    assert_eq!(zone_at(4, 20), Some(ScrollZone::LeftUp));
    assert_eq!(zone_at(20, 4), Some(ScrollZone::UpLeft));
}

#[test]
fn map_and_panel_interior_are_not_scroll_zones() {
    assert_eq!(zone_at(300, 200), None);
    assert_eq!(zone_at(100, 100), None);
    assert_eq!(zone_at(300, 30), None);
    // End Turn extends onto the map, but the click still belongs to the button
    // (`WINDOW_ENDTURN_BUTTON` before `WINDOW_SCROLL_UP_WINDOW` in C++).
    assert_eq!(zone_at(400, 4), Some(ScrollZone::Up));
    assert_eq!(active_zone_at(400, 4), None);
    assert_eq!(active_zone_at(300, 4), Some(ScrollZone::Up));
}

#[test]
fn directions_match_set_grid_offset() {
    assert_eq!(ScrollZone::Up.direction(), (0, -1));
    assert_eq!(ScrollZone::UpRight.direction(), (1, -1));
    assert_eq!(ScrollZone::Right.direction(), (1, 0));
    assert_eq!(ScrollZone::DownRight.direction(), (1, 1));
    assert_eq!(ScrollZone::Down.direction(), (0, 1));
    assert_eq!(ScrollZone::DownLeft.direction(), (-1, 1));
    assert_eq!(ScrollZone::Left.direction(), (-1, 0));
    assert_eq!(ScrollZone::LeftUp.direction(), (-1, -1));
}

#[test]
fn click_scroll_requires_a_held_button() {
    let mut scroll = ScrollState::new();
    scroll.set_pointer(300, 4);

    assert_eq!(scroll.step(1.0 / 30.0, true, 16), None);
    assert!(scroll.step(1.0 / 30.0, false, 16).is_some());

    scroll.press(300, 4);
    assert_eq!(
        scroll.step(1.0 / 30.0, true, 16),
        Some((0, -16)),
            "клик по верхней полосе прокручивает карту вверх"
    );
}

#[test]
fn quick_scroll_sets_the_step_size() {
    let mut scroll = ScrollState::new();
    scroll.set_pointer(636, 200);

    assert_eq!(scroll.step(1.0 / 30.0, false, 16), Some((16, 0)));

    scroll.reset_rate();
    assert_eq!(scroll.step(1.0 / 30.0, false, 64), Some((64, 0)));
}

#[test]
fn fractional_rate_carries_over_between_frames() {
    let mut scroll = ScrollState::new();
    scroll.set_pointer(300, 4);

    // "Scroll Speed" 1 at 60 Hz: 1 * (1/60 * 30) = 0.5 px per frame —
    // no whole step yet, the fractional part is carried over.
    assert_eq!(scroll.step(1.0 / 60.0, false, 1), None);

    // Another half-frame — exactly 1 px has accumulated.
    assert_eq!(scroll.step(1.0 / 60.0, false, 1), Some((0, -1)));
}

#[test]
fn dt_zero_does_not_scroll() {
    let mut scroll = ScrollState::new();
    scroll.set_pointer(300, 4);

    assert_eq!(scroll.step(0.0, false, 128), None);
}

#[test]
fn hold_expires_after_the_pulse() {
    let mut scroll = ScrollState::new();
    scroll.press(300, 4);
    assert!(scroll.is_held());

    scroll.tick(CLICK_HOLD_SECONDS);

    assert!(!scroll.is_held());
    assert_eq!(scroll.step(1.0 / 30.0, true, 16), None);
}

#[test]
fn zone_rects_are_the_cpp_inclusive_rects() {
    assert_eq!(zone_rect(ScrollZone::Up), (50, 0, 541, 9));
    assert_eq!(zone_rect(ScrollZone::Right), (632, 50, 8, 381));
    assert_eq!(zone_rect(ScrollZone::UpLeft), (0, 0, 51, 9));
}
