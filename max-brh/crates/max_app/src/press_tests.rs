use super::*;

fn rect() -> PixelRect {
    PixelRect::new(10, 20, 100, 20)
}

#[test]
fn a_held_press_stays_visible_and_matches_only_its_button() {
    arm(15, 25);

    assert_eq!(point(), Some((15, 25)));
    assert!(pressed(rect()));
    assert!(!pressed(PixelRect::new(200, 20, 100, 20)));

    // Held: drawing frames does not end the press.
    for _ in 0..100 {
        advance();
    }
    assert!(pressed(rect()));

    release();
    // The release pulse survives `RELEASE_FRAMES` frames.
    for _ in 0..RELEASE_FRAMES {
        assert!(pressed(rect()));
        advance();
    }
    assert!(!pressed(rect()));
}

#[test]
fn a_second_press_moves_the_feedback() {
    arm(15, 25);
    arm(250, 25);

    assert!(!pressed(rect()));
    assert!(pressed(PixelRect::new(200, 20, 100, 20)));

    release();
    for _ in 0..RELEASE_FRAMES {
        advance();
    }
    assert_eq!(point(), None);
}
