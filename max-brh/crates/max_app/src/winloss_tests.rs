use max_core::universe::GameUniverse;

use super::*;

#[test]
fn background_index_is_bounded() {
    let mut local = LocalGame::new(crate::local::LocalSetup::default());
    local.outcome = Some(crate::local::LocalOutcome {
        victory: true,
        team: TeamId::RED,
        status: GameUniverse::default().snapshot().evaluate_status(),
        turn_counter: 17,
        player_name: "Tester".to_string(),
    });

    assert!(background_index(&local) < 9);
}

#[test]
fn places_match_the_cpp_table() {
    assert_eq!(PLACES[0], "Eliminated");
    assert_eq!(PLACES[1], "1st place");
    assert_eq!(PLACES[4], "4th place");
}

#[test]
fn ok_button_matches_cpp_rectangle() {
    assert_eq!(OK_RECT, PixelRect::new(293, 458, 72, 19));
    assert!(ok_at(300, 460));
    assert!(!ok_at(10, 10));
}
