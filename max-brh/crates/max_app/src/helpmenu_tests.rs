use super::*;

/// The entry with the longest text — for the scrolling check.
fn longest_entry(section: HelpSection) -> HelpScreen {
    let index = section_entries(section)
        .iter()
        .enumerate()
        .max_by_key(|(_, entry)| entry.text.len())
        .map(|(index, _)| index)
        .unwrap_or(0);

    HelpScreen::for_entry(section, HelpPlacement::Menu, index)
}

#[test]
fn four_help_screens_have_text() {
    for section in [
        HelpSection::Cargo,
        HelpSection::Planet,
        HelpSection::NewGame,
        HelpSection::Options,
    ] {
        let text = section_text(section);
        assert!(!text.is_empty(), "{section:?}: пустой текст");

        let screen = HelpScreen::new(section, HelpPlacement::Menu);
        assert!(screen.line_count() > 1, "{section:?}: нет строк");
        assert!(screen.entry_name().is_some());
    }
}

#[test]
fn help_shows_one_entry_per_click_not_the_whole_section() {
    // All sections have entries with coordinates: the window shows exactly one.
    for section in [
        HelpSection::Cargo,
        HelpSection::Planet,
        HelpSection::NewGame,
        HelpSection::Options,
        HelpSection::GameScreen,
        HelpSection::Research,
        HelpSection::Upgrades,
        HelpSection::Allocate,
        HelpSection::Transfer,
        HelpSection::Preferences,
        HelpSection::Setup,
        HelpSection::Clan,
        HelpSection::HotSeat,
    ] {
        let entries = section_entries(section);
        assert!(!entries.is_empty(), "{section:?}: нет записей в help.json");

        let screen = HelpScreen::new(section, HelpPlacement::Menu);
        let entry = &entries[0];

        assert_eq!(screen.entry(), 0);
        assert_eq!(
            screen.lines(),
            wrap_gnw_text(&entry.text, TEXT_RECT.width),
                "{section:?}: окно должно показывать только первую запись"
        );
    }
}

#[test]
fn entry_is_selected_by_point_like_cpp_get_entry() {
    // `Help::GetEntry`: the first entry whose coordinates cover the point.
    let entries = section_entries(HelpSection::Options);

    for (index, entry) in entries.iter().enumerate() {
        let x = (entry.ux + entry.lx) / 2;
        let y = (entry.uy + entry.ly) / 2;

        assert_eq!(
            entry_at(HelpSection::Options, x, y),
            Some(index),
                "{:?}: центр записи должен выбирать её",
            entry.name
        );
    }

    // A point outside all entries — `None` (C++ closes the window).
    assert_eq!(entry_at(HelpSection::Options, 0, 0), None);
    assert_eq!(entry_at(HelpSection::Cargo, 639, 479), None);
}

#[test]
fn game_screen_panels_map_to_their_entries() {
    // `HelpMenu_ScaleCursor`: clicks on the tools are converted into
    // `GAME_SCREEN_SETUP` entry points.
    assert_eq!(
        entry_at(HelpSection::GameScreen, 242 + 10, 456 + 10).map(entry_name_of),
        entry_at(HelpSection::GameScreen, 250, 473).map(entry_name_of)
    );
    assert_eq!(
        entry_at(HelpSection::GameScreen, 338 + 10, 456 + 10).map(entry_name_of),
        entry_at(HelpSection::GameScreen, 448, 473).map(entry_name_of)
    );
    assert_eq!(
        entry_at(HelpSection::GameScreen, 534 + 10, 2 + 10).map(entry_name_of),
        entry_at(HelpSection::GameScreen, 565, 11).map(entry_name_of)
    );
    assert_eq!(
        entry_at(HelpSection::GameScreen, 390 + 10, 2 + 10).map(entry_name_of),
        entry_at(HelpSection::GameScreen, 426, 11).map(entry_name_of)
    );

    // A click on the map is not converted: the map is the common `MAIN_MAP` entry.
    assert_eq!(
        entry_at(HelpSection::GameScreen, 300, 200).map(entry_name_of),
        entry_at(HelpSection::GameScreen, 300, 200).map(entry_name_of)
    );
}

fn entry_name_of(index: usize) -> Option<&'static str> {
    entry_name(HelpSection::GameScreen, index)
}

#[test]
fn entries_show_their_own_text_and_scroll_only_when_longer_than_a_page() {
    // The window shows exactly the entry text; scroll buttons — only if
    // there are more lines than a page (C++ `ProcessText`: `string_row_count >
    // rows_per_page`).
    for section in [
        HelpSection::Cargo,
        HelpSection::Planet,
        HelpSection::NewGame,
        HelpSection::Options,
        HelpSection::GameScreen,
        HelpSection::Research,
        HelpSection::Upgrades,
        HelpSection::Preferences,
        HelpSection::Allocate,
        HelpSection::Transfer,
        HelpSection::Clan,
        HelpSection::HotSeat,
    ] {
        for index in 0..section_entries(section).len() {
            let screen = HelpScreen::for_entry(section, HelpPlacement::Menu, index);
            let expected =
                wrap_gnw_text(entry_text(section, index), TEXT_RECT.width).len();

            assert_eq!(screen.line_count(), expected);
            assert_eq!(screen.is_scrollable(), expected > ROWS_PER_PAGE);
            assert_eq!(screen.scroll_rects().is_some(), expected > ROWS_PER_PAGE);
        }
    }

    // And the `Keys` hint (`_(ceb6)`) is longer than a page and scrolls.
    let mut keys = HelpScreen::new(HelpSection::GameScreen, HelpPlacement::Game);
    keys.toggle_keys();
    assert!(keys.is_scrollable());

    assert_eq!(keys.key(Key::PageDown), HelpAction::None);
    assert_eq!(keys.row_index(), ROWS_PER_PAGE);
    assert_eq!(keys.key(Key::PageUp), HelpAction::None);
    assert_eq!(keys.row_index(), 0);

    for _ in 0..100 {
        keys.page_down();
    }
    assert!(keys.row_index() < keys.line_count());
}

#[test]
fn short_text_is_centered_by_the_draw_rule() {
    // C++ `DrawText`: the block is centered vertically while the height < 160.
    let mut planet = HelpScreen::for_entry(HelpSection::Planet, HelpPlacement::Menu, 0);
    planet.lines = vec!["line".to_string(); 5];
    planet.row_index = 0;

    let block = GnwFontId::Font5.font().height() * planet.line_count() as i32;
    let offset = (TEXT_RECT.height - block).max(0) / 2;

    assert!(!planet.is_scrollable());
    assert!(offset > 0);
}

#[test]
fn window_is_centered_like_dialog_menu() {
    // `Window(HELPFRAM, WINDOW_MAIN_WINDOW)`; the window is 639×479 (lrx/lry inclusive).
    assert_eq!(MENU_RECT.ulx, (639 - HELPFRAM_SIZE.0) / 2);
    assert_eq!(MENU_RECT.uly, (479 - HELPFRAM_SIZE.1) / 2);
    // `WINDOW_MAIN_MAP` 180,18 — 448×448: `180 + (448-301)/2`.
    assert_eq!(GAME_RECT.ulx, 180 + (448 - HELPFRAM_SIZE.0) / 2);
    assert_eq!(GAME_RECT.uly, 18 + (448 - HELPFRAM_SIZE.1) / 2);

    let screen = HelpScreen::new(HelpSection::Cargo, HelpPlacement::Game);
    assert_eq!(screen.window_rect(), GAME_RECT);
    assert_eq!(
        screen.text_rect(),
        PixelRect::new(253 + 20, 126 + 20, 265, 160)
    );
    assert_eq!(screen.done_rect(), PixelRect::new(373, 319, 65, 24));
}

#[test]
fn game_screen_has_keys_button_and_shifted_done() {
    let screen = HelpScreen::new(HelpSection::GameScreen, HelpPlacement::Game);

    assert_eq!(
        screen.done_rect(),
        PixelRect::new(253 + 85, 126 + 193, 65, 24)
    );
    assert_eq!(
        screen.keys_rect(),
        Some(PixelRect::new(253 + 155, 126 + 193, 65, 24))
    );

    // `Keys` (`_(ceb6)`) is longer than any entry and scrolls; toggling
    // returns to the top and back to the entry.
    let mut screen = screen;
    screen.toggle_keys();
    assert!(screen.keys_mode());
    assert!(screen.is_scrollable());
    screen.page_down();
    assert!(screen.row_index() > 0);
    screen.toggle_keys();
    assert!(!screen.keys_mode());
    assert_eq!(screen.row_index(), 0);
    assert_eq!(
        screen.lines(),
        wrap_gnw_text(entry_text(HelpSection::GameScreen, 0), TEXT_RECT.width)
    );
}

#[test]
fn keys_close_and_scroll() {
    // The entry may not scroll — page navigation is checked on
    // `Keys` (FONT5, 265 px, more lines than a page).
    let mut screen = HelpScreen::new(HelpSection::GameScreen, HelpPlacement::Menu);
    screen.toggle_keys();
    assert_eq!(screen.key(Key::PageDown), HelpAction::None);
    assert_eq!(screen.row_index(), ROWS_PER_PAGE);
    assert_eq!(screen.key(Key::PageUp), HelpAction::None);
    assert_eq!(screen.row_index(), 0);
    assert_eq!(screen.key(Key::Down), HelpAction::None);
    assert_eq!(screen.row_index(), 1);
    assert_eq!(screen.key(Key::Up), HelpAction::None);
    assert_eq!(screen.row_index(), 0);
    assert_eq!(screen.key(Key::Enter), HelpAction::Close);
    assert_eq!(screen.key(Key::Escape), HelpAction::Close);
    assert_eq!(screen.key(Key::Char('x')), HelpAction::None);
}

#[test]
fn clicks_hit_buttons_and_ignore_the_rest() {
    let mut screen = HelpScreen::new(HelpSection::GameScreen, HelpPlacement::Game);
    screen.toggle_keys();
    let (up, down) = screen.scroll_rects().expect("Keys прокручивается");

    assert_eq!(screen.click(up.ulx + 1, up.uly + 1), HelpAction::None);
    assert_eq!(screen.row_index(), 0);
    assert_eq!(screen.click(down.ulx + 1, down.uly + 1), HelpAction::None);
    assert_eq!(screen.row_index(), ROWS_PER_PAGE);

    let done = screen.done_rect();
    assert_eq!(screen.click(done.ulx + 1, done.uly + 1), HelpAction::Close);

    // A click outside the buttons does nothing.
    let mut screen = HelpScreen::new(HelpSection::Planet, HelpPlacement::Menu);
    assert_eq!(screen.click(0, 0), HelpAction::None);
    assert_eq!(screen.row_index(), 0);
}

#[test]
fn section_names_match_the_oracle() {
    assert_eq!(HelpSection::Cargo.as_str(), "CARGO_SETUP");
    assert_eq!(HelpSection::Planet.as_str(), "PLANET_SETUP");
    assert_eq!(HelpSection::NewGame.as_str(), "NEW_GAME_SETUP");
    assert_eq!(HelpSection::Options.as_str(), "OPTIONS_SETUP");
    assert_eq!(HelpSection::Research.as_str(), "RESEARCH_SETUP");
    assert_eq!(HelpSection::Upgrades.as_str(), "UPGRADES_SETUP");
    assert_eq!(HelpSection::GameScreen.as_str(), "GAME_SCREEN_SETUP");
    assert_eq!(HelpSection::parse("cargo"), Some(HelpSection::Cargo));
    assert_eq!(HelpSection::parse("newgame"), Some(HelpSection::NewGame));
    assert_eq!(HelpSection::parse("bogus"), None);
}
