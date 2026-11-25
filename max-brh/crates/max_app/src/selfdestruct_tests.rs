use super::*;

#[test]
fn window_is_centered_in_the_map_area() {
    assert_eq!(DIALOG_RECT.ulx, 180 + (448 - DIALOG_SIZE.0) / 2);
    assert_eq!(DIALOG_RECT.uly, 18 + (448 - DIALOG_SIZE.1) / 2);
    assert_eq!(DIALOG_RECT.width, DIALOG_SIZE.0);
    assert_eq!(DIALOG_RECT.height, DIALOG_SIZE.1);
}

#[test]
fn arm_plays_six_frames_then_shows_ok() {
    let mut menu = SelfDestructMenu::new(UnitId(3));
    assert_eq!(menu.stage, Stage::Confirm);
    assert_eq!(
        menu.input(InputEvent::MouseClick(
            menu.arm_rect().ulx + 2,
            menu.arm_rect().uly + 2
        )),
        SelfDestructAction::None
    );
    assert_eq!(menu.stage, Stage::Armed);
    assert_eq!(menu.frame, 0);
    assert!(!menu.is_armed());

    for _ in 0..OPEN_FRAMES {
        menu.advance(FRAME_SECONDS);
    }
    assert!(menu.is_armed());
    assert_eq!(menu.frame, OPEN_FRAMES - 1);

    assert_eq!(
        menu.input(InputEvent::MouseClick(
            menu.ok_rect().ulx + 2,
            menu.ok_rect().uly + 2
        )),
        SelfDestructAction::Explode
    );
}

#[test]
fn escape_and_cancel_do_not_explode() {
    let mut menu = SelfDestructMenu::new(UnitId(3));

    assert_eq!(
        menu.input(InputEvent::MouseClick(
            menu.cancel_rect().ulx + 2,
            menu.cancel_rect().uly + 2
        )),
        SelfDestructAction::Cancel
    );

    let mut menu = SelfDestructMenu::new(UnitId(3));
    assert_eq!(
        menu.input(InputEvent::Key(Key::Escape)),
        SelfDestructAction::Cancel
    );

    // Enter on the first window — Arm (C++ `SetPValue(GNW_KB_KEY_RETURN)`).
    let mut menu = SelfDestructMenu::new(UnitId(3));
    assert_eq!(
        menu.input(InputEvent::Key(Key::Enter)),
        SelfDestructAction::None
    );
    assert_eq!(menu.stage, Stage::Armed);
    // Until the animation finishes, Enter does not confirm.
    assert_eq!(
        menu.input(InputEvent::Key(Key::Enter)),
        SelfDestructAction::None
    );
}

#[test]
fn frames_are_deterministic_at_fixed_step() {
    let mut first = SelfDestructMenu::new(UnitId(1));
    let mut second = SelfDestructMenu::new(UnitId(1));
    first.arm();
    second.arm();

    for _ in 0..10 {
        first.advance(1.0 / 60.0);
        second.advance(1.0 / 60.0);
    }

    assert_eq!(first.frame, second.frame);
    assert!(first.frame > 0);
}
