use super::*;
use max_assets::mission::{TextBlock, DEFAULT_LANGUAGE};

fn text_block(text: &str) -> TextBlock {
    TextBlock {
        text: std::collections::BTreeMap::from([(
            DEFAULT_LANGUAGE.to_string(),
            text.to_string(),
        )]),
    }
}

fn block(text: &str) -> StoryBlock {
    StoryBlock {
        text: text_block(text),
        background: None,
        video: std::collections::BTreeMap::new(),
        music: None,
    }
}

fn media_ids(id: &str) -> Media {
    Media {
        resource: Some(MediaResource::Ids(vec![id.to_string()])),
        copyright: String::new(),
        license: String::new(),
    }
}

fn media_files(file: &str) -> Media {
    Media {
        resource: Some(MediaResource::Files(vec![file.to_string()])),
        copyright: String::new(),
        license: String::new(),
    }
}

/// The text is wrapped to the 600 px box with `FONT1`; hard `\n` breaks stay.
#[test]
fn the_text_is_wrapped_to_the_oracle_width() {
    let rows = wrap_story_text("one two\n\nthree");

    assert_eq!(
        rows,
        vec!["one two".to_string(), String::new(), "three".to_string()]
    );

    let font = GnwFontId::Font1.font();
    let long = wrap_story_text(&"word ".repeat(400));

    assert!(long.len() > 1);

    for row in &long {
        assert!(font.text_width(row) <= STORY_TEXT_WIDTH, "{row:?}");
    }
}

/// An empty text block with no video is no screen at all (the oracle shows
/// nothing) — the drivers start the mission without any story.
#[test]
fn an_empty_story_is_not_a_screen() {
    let screen = StoryScreen::from_block(
        StoryKind::Intro,
        &block(""),
        DEFAULT_LANGUAGE,
        0,
        None,
        Path::new("."),
    );

    assert!(screen.is_empty());
    assert_eq!(screen.phase(), StoryPhase::Text);
}

/// A story with no data at all still has its text; missing media is visible.
#[test]
fn missing_media_is_a_visible_diagnostic() {
    let mut story = block("The Council has received secret survey reports.");
    story.background = Some(media_ids("ENDGAME1"));
    story.video = std::collections::BTreeMap::from([(
        DEFAULT_LANGUAGE.to_string(),
        media_ids("DEMO2FLC"),
    )]);
    story.music = Some(media_ids("WINR_MSC"));

    let screen = StoryScreen::from_block(
        StoryKind::Intro,
        &story,
        DEFAULT_LANGUAGE,
        0,
        None,
        Path::new("no-such-dir"),
    );

    assert!(!screen.is_empty(), "the text is shown without media");
    assert!(!screen.has_background());
    assert!(screen.videos().is_empty());
    assert_eq!(screen.music(), Some("WINR_MSC"));
    assert!(
        screen
            .diagnostics()
            .iter()
            .any(|line| line.contains("MAX.RES")),
            "{:?}",
        screen.diagnostics()
    );

    // Without a video the text window closes straight into the driver action.
    assert_eq!(
        screen
            .clone()
            .input(crate::ui::InputEvent::Key(crate::ui::Key::Enter)),
        StoryAction::Finished
    );
}

/// A file-form video that is missing is a diagnostic, and Enter finishes the
/// story (a missing story file never blocks the mission start).
#[test]
fn a_missing_story_file_does_not_block_the_screen() {
    let mut story = block("Story text.");
    story.video = std::collections::BTreeMap::from([(
        DEFAULT_LANGUAGE.to_string(),
        media_files("no-such-movie.mve"),
    )]);

    let mut screen = StoryScreen::from_block(
        StoryKind::Victory,
        &story,
        DEFAULT_LANGUAGE,
        0,
        None,
        Path::new("no-such-dir"),
    );

    assert!(screen.videos().is_empty());
    assert!(
        screen
            .diagnostics()
            .iter()
            .any(|line| line.contains("no-such-movie.mve")),
            "{:?}",
        screen.diagnostics()
    );
    assert_eq!(
        screen.input(crate::ui::InputEvent::Key(crate::ui::Key::Enter)),
        StoryAction::Finished
    );
    assert_eq!(screen.phase(), StoryPhase::Text);
}

/// The text paginates: every row is reachable, the first page is 23 rows.
#[test]
fn long_text_paginates() {
    let text = (0..60)
        .map(|index| format!("row {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut screen = StoryScreen::from_block(
        StoryKind::Victory,
        &block(&text),
        DEFAULT_LANGUAGE,
        0,
        None,
        Path::new("."),
    );

    assert_eq!(StoryScreen::rows_per_page(), 23);
    assert_eq!(screen.page_count(), 3);
    assert_eq!(screen.visible_rows().len(), 23);

    screen.input(crate::ui::InputEvent::Key(crate::ui::Key::PageDown));
    assert_eq!(screen.page(), 2);
    assert_eq!(
        screen.visible_rows().last().map(String::as_str),
        Some("row 59")
    );
    screen.input(crate::ui::InputEvent::Key(crate::ui::Key::Up));
    assert_eq!(screen.page(), 1);
}

/// `Text_TypeWriter_CharacterTimeMs = 10` (`menu.cpp:795`): the page is revealed
/// character by character, any input completes it (and is consumed), the next
/// input closes the text window.
#[test]
fn the_story_text_is_typed_character_by_character() {
    let mut screen = StoryScreen::from_block(
        StoryKind::Intro,
        &block("The Secret Council."),
        DEFAULT_LANGUAGE,
        0,
        None,
        Path::new("."),
    );
    let total: usize = screen
        .visible_rows()
        .iter()
        .map(|row| row.chars().count())
        .sum();

    assert!(total > 5);
    assert!(!screen.typing(), "a dt = 0 frame draws the complete page");
    assert_eq!(screen.typed_chars(), 0);

    // 3 characters at 10 ms each (the first positive dt starts the reveal).
    assert!(!screen.advance(0.03));
    assert!(screen.typing());
    assert_eq!(screen.typed_chars(), 3);

    // Any key while typing only completes the page and is consumed.
    assert_eq!(
        screen.input(crate::ui::InputEvent::Key(crate::ui::Key::Char('x'))),
        StoryAction::None
    );
    assert_eq!(screen.typed_chars(), total);
    assert!(!screen.typing());

    // The following Enter closes the text (there is no video in this block).
    assert_eq!(
        screen.input(crate::ui::InputEvent::Key(crate::ui::Key::Enter)),
        StoryAction::Finished
    );
}

/// A page turn starts a new typewriter pass once the animation is running; a
/// `dt = 0` freeze-frame keeps whole pages deterministic.
#[test]
fn a_page_turn_restarts_the_typewriter() {
    let text = (0..60)
        .map(|index| format!("row {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut screen = StoryScreen::from_block(
        StoryKind::Victory,
        &block(&text),
        DEFAULT_LANGUAGE,
        0,
        None,
        Path::new("."),
    );

    screen.advance(0.05);
    assert!(screen.typing());

    // The first PageDown only completes the current page (consumed input).
    screen.input(crate::ui::InputEvent::Key(crate::ui::Key::PageDown));
    assert_eq!(screen.page(), 0);
    assert!(!screen.typing());

    // The next one turns the page; the new page types from its start.
    screen.input(crate::ui::InputEvent::Key(crate::ui::Key::PageDown));
    assert_eq!(screen.page(), 2);
    assert_eq!(screen.typed_chars(), 0, "the new page types from its start");
    assert!(screen.typing());

    screen.advance(0.02);
    assert_eq!(screen.typed_chars(), 2);
}

/// The drawing is deterministic (`dt = 0` frames) even without `MAX.RES`/menu.
#[test]
fn drawing_without_assets_is_deterministic_and_visible() {
    let screen = StoryScreen::from_block(
        StoryKind::Intro,
        &block("The Secret Council of Freedom."),
        DEFAULT_LANGUAGE,
        3,
        None,
        Path::new("."),
    );
    let mut first = RgbaBuffer::new(640, 480);
    let mut second = RgbaBuffer::new(640, 480);

    assert!(screen.draw(&mut first, None, None) > 0);
    screen.draw(&mut second, None, None);

    assert_eq!(first.pixels(), second.pixels());
    assert!(ok_at(320, 466));
}

/// The deterministic background index stays inside the nine briefing pictures.
#[test]
fn the_briefing_background_index_is_bounded() {
    for seed in 0..40 {
        let screen = StoryScreen::from_block(
            StoryKind::Intro,
            &block("x"),
            DEFAULT_LANGUAGE,
            seed,
            None,
            Path::new("."),
        );

        assert!(screen.background_index < BRIEFING_BACKGROUNDS.len());
    }
}
