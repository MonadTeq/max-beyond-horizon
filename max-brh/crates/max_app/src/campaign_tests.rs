use super::*;

/// `DrawMissionList`/`EventSelectItem` bound: a mission is unlocked while its
/// 0-based index is below `last_campaign`.
#[test]
fn campaign_list_opens_on_last_campaign_and_limits_the_unlocked_rows() {
    let open = menu_open(3, 9);
    assert_eq!(open.last_campaign, 3);
    assert_eq!(open.selected, 2, "`game_file_number - 1`");
    assert!(!open.clamped);
}

/// `menu.cpp:1970-1982`: a value above the list is clamped down and reported for
/// the write-back; a value below zero unlocks nothing.
#[test]
fn campaign_open_clamps_only_above_the_list() {
    let above = menu_open(99, 9);
    assert_eq!(above.last_campaign, 9);
    assert_eq!(above.selected, 8);
    assert!(above.clamped, "the oracle writes the clamp back");

    let exact = menu_open(9, 9);
    assert_eq!(exact.last_campaign, 9);
    assert!(!exact.clamped, "no write-back when the value fits");

    let below = menu_open(-4, 9);
    assert_eq!(below.last_campaign, 0, "negative: nothing unlocked");
    assert_eq!(below.selected, 0);
    assert!(!below.clamped);

    let empty = menu_open(4, 0);
    assert_eq!(empty.last_campaign, 0);
    assert_eq!(empty.selected, 0);
    assert!(empty.clamped, "a non-empty progression over an empty list");
}

/// `menu.cpp:518-545`: only a mission that has a successor advances the values;
/// `last_campaign` is a high-water mark and never decreases.
#[test]
fn victory_advance_moves_to_the_next_mission_and_keeps_the_high_water_mark() {
    let mut prefs = Preferences::default();
    assert_eq!(prefs.last_campaign, 1);

    let advance = victory_advance(0, 9, &prefs).expect("mission 2 exists");
    assert_eq!(advance.game_file_number, 2, "1-based next mission");
    assert_eq!(advance.last_campaign, 2);

    prefs.last_campaign = 5;
    let replay = victory_advance(0, 9, &prefs).expect("mission 2 exists");
    assert_eq!(replay.game_file_number, 2);
    assert_eq!(
        replay.last_campaign, 5,
            "the high-water mark does not shrink"
    );

    assert!(
        victory_advance(8, 9, &prefs).is_none(),
            "the last mission has no successor: the settings stay untouched"
    );
    assert!(victory_advance(0, 1, &prefs).is_none());
}
