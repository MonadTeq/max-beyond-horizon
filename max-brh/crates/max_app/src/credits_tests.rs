use super::*;

#[test]
fn data_has_171_lines_in_menu_cpp_order() {
    assert_eq!(line_count(), 171, "menu_credits_lines: 171 непустая строка");
    assert_eq!(lines()[0].text, "Project Manager");
    assert_eq!(lines()[0].color, 0xFF);
    assert_eq!(lines()[1].text, "Ali Atabek");
    assert_eq!(lines()[1].color, 0x04);
    assert_eq!(lines()[2].text, "Producers");
    // Credits at the end of the table (Parallax Software — installer).
    assert!(lines().iter().any(|line| line.text.contains("Parallax")));
    assert!(lines().iter().all(|line| !line.text.is_empty()));
}

#[test]
fn dt_zero_keeps_the_screen_still() {
    let mut screen = CreditsScreen::new();

    for _ in 0..600 {
        screen.advance(0.0);
    }

    assert_eq!(screen.steps(), 0);
    assert_eq!(screen.line_index(), 0);
    assert!(!screen.finished());
    assert!(screen.visible_lines().is_empty());
}

#[test]
fn one_second_scrolls_one_hundred_steps_at_default_speed() {
    let mut screen = CreditsScreen::new();

    for _ in 0..60 {
        screen.advance(1.0 / 60.0);
    }

    // 10 ms per step: 99-100 steps per second (microsecond rounding).
    assert!(
        matches!(screen.steps(), 99 | 100),
            "шагов: {}",
        screen.steps()
    );
    // In 100 steps 6 lines appear (step 20) = line_index 6.
    assert!(
        matches!(screen.line_index(), 5 | 6),
            "{}",
        screen.line_index()
    );
}

#[test]
fn lines_appear_every_twenty_steps_and_scroll_up() {
    let mut screen = CreditsScreen::new();

    // Step 1: the first line is drawn at line 231 (still below the window).
    screen.advance(0.01);

    assert_eq!(screen.steps(), 1);
    assert_eq!(screen.line_index(), 1);
    assert_eq!(screen.row_of(0), TEXT_ROW);
    assert!(screen.visible_lines().is_empty());

    // Step 2: the line at 230 does not fit into the window yet (lines 0..229 are visible).
    screen.advance(0.01);
    assert_eq!(screen.row_of(0), TEXT_ROW - 1);
    assert!(screen.visible_lines().is_empty());

    // Step 3: the line at 229 — the bottom visible line of the window.
    screen.advance(0.01);
    assert_eq!(screen.visible_lines(), vec![(0, TEXT_ROW - 2)]);

    // By step 21 a second line was added, the first moved up by 20.
    for _ in 0..18 {
        screen.advance(0.01);
    }

    assert_eq!(screen.steps(), 21);
    assert_eq!(screen.line_index(), 2);
    assert_eq!(screen.row_of(0), TEXT_ROW - 20);
    assert_eq!(screen.row_of(1), TEXT_ROW);
    // The second line does not fit into the window yet (line 231 > 229).
    assert_eq!(screen.visible_lines(), vec![(0, TEXT_ROW - 20)]);

    for _ in 0..20 {
        screen.advance(0.01);
    }

    assert_eq!(screen.steps(), 41);
    assert_eq!(screen.row_of(2), TEXT_ROW);
    // The third line is still below the window — only the first two are visible.
    assert_eq!(
        screen.visible_lines(),
        vec![(0, TEXT_ROW - 40), (1, TEXT_ROW - 20)]
    );
}

#[test]
fn screen_finishes_after_the_last_line_leaves_the_top() {
    let mut screen = CreditsScreen::new();
    // The last line is drawn at step `1 + 20·(N-1)`, then 250 steps,
    // until `height_offset` goes from 375 to 125 (`bounds.uly`).
    let total = 1 + 20 * (line_count() as u32 - 1) + 250;

    // We go through to the end: 3651 steps at 100 steps/s.
    for _ in 0..(total * 11) {
        screen.advance(0.01);
    }

    assert!(screen.finished(), "экран должен закрыться сам");
    assert_eq!(screen.line_index(), line_count());
    assert!(screen.height_offset() <= CREDITS_ULY);
    assert!(
        screen.steps() >= total,
            "шагов {} < {total}",
        screen.steps()
    );
    // After closing, no steps run.
    let steps = screen.steps();
    screen.advance(1.0);
    assert_eq!(screen.steps(), steps);
}

#[test]
fn speed_changes_by_nine_like_the_keypad_controls() {
    let mut screen = CreditsScreen::new();

    assert_eq!(screen.scroll_speed(), 100);

    screen.faster();
    assert_eq!(screen.scroll_speed(), 109);

    for _ in 0..200 {
        screen.faster();
    }
    assert_eq!(screen.scroll_speed(), MAX_SPEED);

    screen.slower();
    assert_eq!(screen.scroll_speed(), MAX_SPEED - SPEED_RATE);

    for _ in 0..200 {
        screen.slower();
    }
    // Stops at the first speed less than a step (as in C++:
    // `if (scroll_speed >= scroll_rate) scroll_speed -= scroll_rate`).
    assert!(
        screen.scroll_speed() < SPEED_RATE,
            "{}",
        screen.scroll_speed()
    );
    assert!(screen.scroll_speed() > 0);

    // Speed affects scrolling: 109 steps/s → 1 step per 10 ms.
    let mut fast = CreditsScreen::new();
    fast.faster();
    fast.advance(0.01);
    assert_eq!(fast.steps(), 1);
}

#[test]
fn draw_paints_background_and_text_only_inside_the_region() {
    let menu =
        crate::menu::MenuAssets::load(&max_assets::ResArchive::open(repo_max_res()).unwrap());

    let Some(menu) = menu else {
        eprintln!("пропуск: нет MAX.RES");
        return;
    };

    let Some(background) = menu.credits.as_ref() else {
        eprintln!("пропуск: нет CREDITS");
        return;
    };

    let mut screen = CreditsScreen::new();
    let mut frame = RgbaBuffer::new(640, 480);
    let painted = screen.draw(&mut frame, background);

    assert!(painted >= 640 * 480, "фон должен занять весь кадр");

    // At the very beginning the text is not visible yet — the area matches the background.
    let area = |frame: &RgbaBuffer, y0: i32, y1: i32| {
        let mut out = Vec::new();

        for y in y0..y1 {
            for x in CREDITS_ULX..(CREDITS_ULX + CREDITS_WIDTH) {
                out.push(frame.get(x, y));
            }
        }

        out
    };

    let before = area(&frame, CREDITS_ULY, CREDITS_ULY + CREDITS_HEIGHT);

    for _ in 0..30 {
        screen.advance(0.01);
    }

    let mut with_text = RgbaBuffer::new(640, 480);
    screen.draw(&mut with_text, background);
    let after = area(&with_text, CREDITS_ULY, CREDITS_ULY + CREDITS_HEIGHT);

    assert_ne!(before, after, "строки титров должны изменить область");

    // Nothing changes outside the area.
    for y in 0..480 {
        for x in 0..640 {
            let inside = (CREDITS_ULX..CREDITS_ULX + CREDITS_WIDTH).contains(&x)
                && (CREDITS_ULY..CREDITS_ULY + CREDITS_HEIGHT).contains(&y);

            if !inside {
                assert_eq!(
                    frame.get(x, y),
                    with_text.get(x, y),
                        "пиксель вне области ({x},{y}) изменился"
                );
            }
        }
    }
}

fn repo_max_res() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("max-original")
        .join("MAX.RES")
}
