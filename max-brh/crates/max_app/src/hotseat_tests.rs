use super::*;

#[test]
fn dialog_is_centered_in_the_main_map() {
    // `((627 - 180 - 301) / 2) + 180 = 253`, `((465 - 18 - 231) / 2) + 18 = 126`.
    assert_eq!(DIALOG_RECT, PixelRect::new(253, 126, 301, 231));
    assert_eq!(
        MAIN_MAP_RECT.ulx + (MAIN_MAP_RECT.width - HELPFRAM_SIZE.0) / 2,
        253
    );
    assert_eq!(
        MAIN_MAP_RECT.uly + (MAIN_MAP_RECT.height - HELPFRAM_SIZE.1) / 2,
        126
    );
}

#[test]
fn ok_button_matches_the_dialog_menu_controls() {
    // `Button(HELPOK_U, HELPOK_D, 120, 193)` 65×24 inside the dialog window.
    assert_eq!(OK_RECT, PixelRect::new(120, 193, 65, 24));
    assert_eq!(ok_rect_frame(), PixelRect::new(373, 319, 65, 24));
    assert!(ok_at(380, 325));
    assert!(!ok_at(253, 126), "угол окна — не кнопка");
}

#[test]
fn turn_message_matches_the_oracle_string() {
    assert_eq!(turn_message(TeamId::RED), "Red Team:\nBegin turn.");
    assert_eq!(turn_message(TeamId::GREEN), "Green Team:\nBegin turn.");
    assert_eq!(turn_message(TeamId::BLUE), "Blue Team:\nBegin turn.");
    assert_eq!(turn_message(TeamId::GRAY), "Gray Team:\nBegin turn.");

    let notice = TurnNotice::new(TeamId::GREEN);
    assert_eq!(notice.lines(), vec!["Green Team:", "Begin turn."]);
}

#[test]
fn text_block_is_vertically_centered() {
    // Two lines of 10 px → `(175 - 20) / 2 = 77` from the top of the text area.
    let text = text_rect_frame();
    let block = 10 * TurnNotice::new(TeamId::RED).lines().len() as i32;
    let y = text.uly + (TEXT_RECT.height - block).max(0) / 2;

    assert_eq!(text, PixelRect::new(273, 140, 265, 175));
    assert_eq!(y, 140 + 77);
}

#[test]
fn human_teams_follow_the_team_slot_order() {
    let teams = human_teams(&[
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::Player,
        TeamKind::None,
    ]);

    assert_eq!(teams, vec![TeamId::RED, TeamId::BLUE]);
}
