use super::*;

#[test]
fn pause_dialog_has_two_centered_lines_and_ok() {
    let dialog = DialogMenu::pause();

    assert_eq!(dialog.lines(), &["Game Paused.", "Click OK to continue."]);
    assert!(!dialog.is_scrollable());
    assert_eq!(dialog.window_rect(), pause_window());
    assert_eq!(
        dialog.ok_rect(),
        PixelRect::new(253 + 120, 126 + 193, 65, 24)
    );
    assert_eq!(
        dialog,
        DialogMenu::new(PAUSE_TEXT, HelpPlacement::Game, true)
    );
}

#[test]
fn ok_enter_escape_and_click_close_the_dialog() {
    let mut dialog = DialogMenu::pause();
    let ok = dialog.ok_rect();

    assert_eq!(
        dialog.input(InputEvent::Key(Key::Enter)),
        DialogAction::Close
    );
    assert_eq!(
        dialog.input(InputEvent::Key(Key::Escape)),
        DialogAction::Close
    );
    assert_eq!(
        dialog.input(InputEvent::MouseClick(ok.ulx + 2, ok.uly + 2)),
        DialogAction::Close
    );
    // A click outside the button does nothing.
    assert_eq!(
        dialog.input(InputEvent::MouseClick(0, 0)),
        DialogAction::None
    );
}

#[test]
fn long_text_gets_scroll_buttons() {
    let text = (0..40)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut dialog = DialogMenu::new(&text, HelpPlacement::Game, false);

    assert!(dialog.is_scrollable());
    let (up, down) = dialog.scroll_rects().expect("прокрутка");

    assert_eq!(
        dialog.input(InputEvent::MouseClick(down.ulx + 1, down.uly + 1)),
        DialogAction::None
    );
    assert_eq!(dialog.row_index(), MAX_ROWS);
    assert_eq!(
        dialog.input(InputEvent::MouseClick(up.ulx + 1, up.uly + 1)),
        DialogAction::None
    );
    assert_eq!(dialog.row_index(), 0);

    dialog.page_down();
    assert_eq!(dialog.row_index(), MAX_ROWS);
}
