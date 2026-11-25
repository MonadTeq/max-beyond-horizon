use super::*;

fn record(recorder: &mut Recorder, event: InputEvent) {
    recorder.record(
        event,
        RecordContext {
            in_session: false,
            text_open: false,
        },
    );
}

fn click(recorder: &mut Recorder, x: i32, y: i32) {
    record(recorder, InputEvent::MouseClick(x, y));
}

#[test]
fn clicks_are_written_in_frame_pixels_and_waits_count_frames() {
    let mut recorder = Recorder::in_memory();

    click(&mut recorder, 10, 20);
    recorder.begin_frame();
    recorder.begin_frame();
    click(&mut recorder, 30, 40);
    record(&mut recorder, InputEvent::RightClick(0, 0));

    assert_eq!(
        recorder.commands(),
        [
                "click 10 20",
                "wait 2",
                "click 30 40",
                "rclick 0 0"
        ]
    );
    // The first event — without wait: pauses are written only between events.
    assert_eq!(recorder.events(), 3);
}

#[test]
fn out_of_frame_clicks_and_mouse_motion_are_dropped() {
    let mut recorder = Recorder::in_memory();

    record(&mut recorder, InputEvent::MouseMove(100, 100));
    click(&mut recorder, -1, -1);
    click(&mut recorder, FRAME_WIDTH, 10);
    click(&mut recorder, 10, FRAME_HEIGHT);
    click(&mut recorder, -5, 20);

    assert!(recorder.commands().is_empty());
    assert_eq!(recorder.events(), 0);
}

#[test]
fn named_keys_and_printable_chars_become_key_commands() {
    let mut recorder = Recorder::in_memory();

    for key in [
        Key::Enter,
        Key::Escape,
        Key::Tab,
        Key::Backspace,
        Key::Up,
        Key::Down,
        Key::Left,
        Key::Right,
        Key::Char('a'),
        Key::Char('T'),
    ] {
        record(&mut recorder, InputEvent::Key(key));
    }
    // Outside text input the space and other keys are not expressible.
    record(&mut recorder, InputEvent::Key(Key::Char(' ')));
    record(&mut recorder, InputEvent::Key(Key::Other));

    assert_eq!(
        recorder.commands(),
        [
                "key ENTER",
                "key ESC",
                "key TAB",
                "key BACKSPACE",
                "key UP",
                "key DOWN",
                "key LEFT",
                "key RIGHT",
                "key a",
                "key T"
        ]
    );
}

#[test]
fn printable_text_between_open_and_enter_is_one_type_command() {
    let mut recorder = Recorder::in_memory();
    let session = RecordContext {
        in_session: true,
        text_open: false,
    };

    // Enter in a session opens the chat — this is a separate key.
    recorder.record(InputEvent::Key(Key::Enter), session);
    let open = RecordContext {
        in_session: true,
        text_open: true,
    };

    for character in "Hello, world 42".chars() {
        recorder.record(InputEvent::Key(Key::Char(character)), open);
    }

    // Enter sends the message: type is closed, then key ENTER.
    recorder.record(InputEvent::Key(Key::Enter), open);

    assert_eq!(
        recorder.commands(),
        ["key ENTER", "type \"Hello, world 42\"", "key ENTER"]
    );
}

#[test]
fn text_is_flushed_before_escape_and_backspace_splits_type() {
    let mut recorder = Recorder::in_memory();
    let open = RecordContext {
        in_session: true,
        text_open: true,
    };

    for character in "abc".chars() {
        recorder.record(InputEvent::Key(Key::Char(character)), open);
    }
    recorder.record(InputEvent::Key(Key::Backspace), open);
    for character in "d".chars() {
        recorder.record(InputEvent::Key(Key::Char(character)), open);
    }
    recorder.record(InputEvent::Key(Key::Escape), open);

    assert_eq!(
        recorder.commands(),
        [
                "type \"abc\"",
                "key BACKSPACE",
                "type \"d\"",
                "key ESC"
        ]
    );
}

#[test]
fn wait_before_type_and_before_closing_key_preserve_pauses() {
    let mut recorder = Recorder::in_memory();
    let session = RecordContext {
        in_session: true,
        text_open: false,
    };
    let open = RecordContext {
        in_session: true,
        text_open: true,
    };

    recorder.record(InputEvent::Key(Key::Char('t')), session);
    for _ in 0..5 {
        recorder.begin_frame();
    }
    for character in "hi".chars() {
        recorder.record(InputEvent::Key(Key::Char(character)), open);
    }
    for _ in 0..3 {
        recorder.begin_frame();
    }
    recorder.record(InputEvent::Key(Key::Enter), open);

    assert_eq!(
        recorder.commands(),
        ["key t", "wait 5", "type \"hi\"", "wait 3", "key ENTER"]
    );
}

#[test]
fn long_pauses_are_capped() {
    let mut recorder = Recorder::in_memory().with_pause_cap(120);

    click(&mut recorder, 1, 2);
    for _ in 0..900 {
        recorder.begin_frame();
    }
    click(&mut recorder, 5, 6);

    assert_eq!(
        recorder.commands(),
        ["click 1 2", "wait 120", "click 5 6"]
    );
}

#[test]
fn zero_cap_disables_waits() {
    let mut recorder = Recorder::in_memory().with_pause_cap(0);

    click(&mut recorder, 1, 2);
    for _ in 0..10 {
        recorder.begin_frame();
    }
    click(&mut recorder, 3, 4);

    assert_eq!(recorder.commands(), ["click 1 2", "click 3 4"]);
}

#[test]
fn finish_flushes_open_text_and_trailing_wait() {
    let mut recorder = Recorder::in_memory();
    let open = RecordContext {
        in_session: true,
        text_open: true,
    };

    recorder.record(InputEvent::Key(Key::Char('h')), open);
    recorder.record(InputEvent::Key(Key::Char('i')), open);
    for _ in 0..4 {
        recorder.begin_frame();
    }

    let path = recorder.finish().expect("finish");

    assert!(path.is_none());
    assert_eq!(recorder.commands(), ["type \"hi\"", "wait 4"]);
}

#[test]
fn typical_session_reads_like_a_qa_script() {
    let mut recorder = Recorder::in_memory();

    // Main menu → New Game.
    click(&mut recorder, 485, 189);
    for _ in 0..2 {
        recorder.begin_frame();
    }
    // Options: No Limit + Turn Based → Done.
    click(&mut recorder, 240, 110);
    click(&mut recorder, 500, 100);
    click(&mut recorder, 516, 440);
    for _ in 0..2 {
        recorder.begin_frame();
    }
    // Planet → Done.
    click(&mut recorder, 516, 440);
    for _ in 0..2 {
        recorder.begin_frame();
    }
    // Players: RED — human, GREEN — computer, Done.
    click(&mut recorder, 202, 102);
    click(&mut recorder, 312, 194);
    click(&mut recorder, 516, 440);
    for _ in 0..2 {
        recorder.begin_frame();
    }
    // In a match: selection, move order, End Turn.
    click(&mut recorder, 404, 242);
    for _ in 0..3 {
        recorder.begin_frame();
    }
    record(&mut recorder, InputEvent::RightClick(468, 306));
    for _ in 0..2 {
        recorder.begin_frame();
    }
    click(&mut recorder, 426, 12);

    assert_eq!(
        recorder.commands(),
        [
                "click 485 189",
                "wait 2",
                "click 240 110",
                "click 500 100",
                "click 516 440",
                "wait 2",
                "click 516 440",
                "wait 2",
                "click 202 102",
                "click 312 194",
                "click 516 440",
                "wait 2",
                "click 404 242",
                "wait 3",
                "rclick 468 306",
                "wait 2",
                "click 426 12"
        ]
    );
}

#[test]
fn existing_file_is_not_overwritten() {
    let dir = std::env::temp_dir().join(format!(
            "max_qa_record_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("run.qa");

    let mut first = Recorder::new(&path).expect("first");
    click(&mut first, 1, 2);
    let first_path = first.finish().expect("finish").expect("path");
    assert_eq!(first_path, path);
    assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), {
        first.header().len() + first.commands().len() + 1
    });

    let mut second = Recorder::new(&path).expect("second");
    click(&mut second, 3, 4);
    let second_path = second.finish().expect("finish").expect("path");
    assert_eq!(second_path, dir.join("run-1.qa"));
    assert!(second_path.is_file());

    // Both files are valid scripts for qa::script.
    for file in [&first_path, &second_path] {
        let text = std::fs::read_to_string(file).expect("read");
        let steps = crate::qa::script::parse_script(&text).expect("parse");
        assert!(!steps.is_empty(), "{}", file.display());
    }

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn header_mentions_manual_recording_and_build() {
    let recorder = Recorder::in_memory();
    let header = recorder.header().join("\n");

    assert!(header.contains("записан вручную"), "{header}");
    assert!(header.contains("дата:"), "{header}");
    assert!(header.contains("билд:"), "{header}");
    assert!(
        header.lines().all(|line| line.starts_with('#')),
            "{header}"
    );
}

#[test]
fn date_conversion_matches_known_dates() {
    assert_eq!(civil_from_days(0), (1970, 1, 1));
    assert_eq!(civil_from_days(19_723), (2024, 1, 1));
    assert_eq!(civil_from_days(20_686), (2026, 8, 21));
    assert!(utc_timestamp().ends_with(" UTC"));
}
