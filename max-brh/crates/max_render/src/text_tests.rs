use super::*;

fn painted_pixels(buffer: &RgbaBuffer) -> usize {
    buffer
        .pixels()
        .chunks_exact(4)
        .filter(|pixel| pixel[3] != 0)
        .count()
}

#[test]
fn table_covers_ascii_and_falls_back_outside_it() {
    assert_eq!(FONT_8X8.len(), ASCII_CHARS);
    assert_eq!(FONT_8X8[0], [0; 8]);
    assert_eq!(FONT_8X8[0x7F], [0; 8], "DEL пустой");
    assert_eq!(glyph('A'), &FONT_8X8[usize::from(b'A')]);
    assert_eq!(glyph('?'), &FONT_8X8[usize::from(b'?')]);
    assert_eq!(glyph('Ж'), glyph('?'), "не-ASCII → запасной символ");
    assert_eq!(glyph('a'), &FONT_8X8[usize::from(b'a')]);
}

#[test]
fn line_width_is_monospace() {
    assert_eq!(char_width(), 8);
    assert_eq!(line_width(""), 0);
    assert_eq!(line_width("MAX"), 24);
    assert_eq!(line_width("A-B"), 24);
    assert_eq!(text_height(3), 24);
}

#[test]
fn wrap_splits_words_and_long_words() {
    assert_eq!(wrap_text("HELLO WORLD", 40), vec!["HELLO", "WORLD"]);
    assert_eq!(wrap_text("ABCDEFG", 24), vec!["ABC", "DEF", "G"]);
    assert_eq!(wrap_text("A\nB", 64), vec!["A", "B"]);
    assert_eq!(wrap_text("", 64), vec![""]);
    assert_eq!(wrap_text("TOO WIDE", 0), Vec::<String>::new());
}

#[test]
fn draw_text_wraps_by_the_buffer_edge() {
    let mut buffer = RgbaBuffer::new(24, 24);
    let metrics = draw_text(&mut buffer, 0, 0, "ABCDE", Rgba::WHITE);

    assert_eq!(metrics.lines, 2);
    assert_eq!(metrics.height, 16);
    assert_eq!(metrics.width, 24, "ширина — самая длинная строка");
    assert!(painted_pixels(&buffer) > 0);
    // 'A' (0x0C = bits 2 and 3) — pixels (2, 0) and (3, 0) in the first row.
    assert_eq!(buffer.get(2, 0), Some(Rgba::WHITE));
    assert_eq!(buffer.get(3, 0), Some(Rgba::WHITE));
    // 'D' — already in the second row (D = 0x1F, 0x36, …).
    assert_eq!(buffer.get(3, 8), Some(Rgba::WHITE));
    assert_eq!(buffer.get(24, 0), None, "за правым краем буфера");
}

/// Glyph orientation: bit 0 is the left pixel. A regression here produced
/// mirrored text across the whole UI ('N' → 'И', 'e' → 'ǝ').
#[test]
fn glyph_bits_are_lsb_first() {
    let mut buffer = RgbaBuffer::new(8, 8);
    // 'E' = 0x7F in the top row → pixels 0..=6, the right one (7) is empty.
    draw_text_line(&mut buffer, 0, 0, "E", Rgba::WHITE);

    assert_eq!(buffer.get(0, 0), Some(Rgba::WHITE), "бит 0 — левый пиксель");
    assert_eq!(buffer.get(6, 0), Some(Rgba::WHITE));
    assert_ne!(buffer.get(7, 0), Some(Rgba::WHITE), "бит 7 — правый");

    // 't' (0x08 = bit 3): the stem is left of center, not right.
    let mut buffer = RgbaBuffer::new(8, 8);
    draw_text_line(&mut buffer, 0, 0, "t", Rgba::WHITE);
    assert_eq!(
        buffer.get(3, 0),
        Some(Rgba::WHITE),
            "'t': бит 3 → колонка 3"
    );
    assert_ne!(buffer.get(4, 0), Some(Rgba::WHITE), "зеркальный 't' → 'ʇ'");
}

#[test]
fn drawing_clips_without_panicking() {
    let mut buffer = RgbaBuffer::new(8, 8);

    draw_text(&mut buffer, -100, -100, "HELLO", Rgba::WHITE);
    draw_text(&mut buffer, 100, 100, "HELLO", Rgba::WHITE);
    draw_text_line(&mut buffer, 4, 4, "M.A.X.", Rgba::WHITE);
    draw_text_wrapped(&mut buffer, 0, 0, -8, "HELLO", Rgba::WHITE);

    assert!(
        painted_pixels(&buffer) > 0,
            "частично видимый текст рисуется"
    );
    assert!(painted_pixels(&buffer) <= buffer.len());
}

#[test]
fn empty_text_draws_nothing() {
    let mut buffer = RgbaBuffer::new(16, 16);
    let metrics = draw_text(&mut buffer, 2, 2, "", Rgba::WHITE);

    assert_eq!(metrics, TextMetrics::default());
    assert_eq!(painted_pixels(&buffer), 0);
}
