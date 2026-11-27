use super::*;

fn count_color(buffer: &RgbaBuffer, color: Rgba) -> usize {
    buffer
        .pixels()
        .chunks_exact(4)
        .filter(|pixel| pixel[..3] == color.to_rgba()[..3])
        .count()
}

#[test]
fn rect_geometry_and_hit_testing() {
    let rect = Rect::new(10, 20, 30, 40);

    assert_eq!(rect.right(), 40);
    assert_eq!(rect.bottom(), 60);
    assert_eq!(rect.center(), Point::new(25, 40));
    assert!(rect.contains(Point::new(10, 20)));
    assert!(!rect.contains(Point::new(40, 20)));
    assert!(!rect.contains(Point::new(10, 60)));
    assert!(Rect::new(0, 0, 0, 5).is_empty());
    assert_eq!(rect.inset(2, 3), Rect::new(12, 23, 26, 34));
    assert_eq!(
        rect.intersect(Rect::new(0, 0, 20, 30)),
        Some(Rect::from_corners(10, 20, 20, 30))
    );
    assert_eq!(rect.intersect(Rect::new(0, 0, 5, 5)), None);
}

#[test]
fn vertical_layout_respects_gap_and_area() {
    let rects = layout(Rect::new(5, 7, 100, 200), &[20, 30, 10], 4);

    assert_eq!(rects.len(), 3);
    assert_eq!(rects[0], Rect::new(5, 7, 100, 20));
    assert_eq!(rects[1], Rect::new(5, 31, 100, 30));
    assert_eq!(rects[2], Rect::new(5, 65, 100, 10));

    let uniform = layout_uniform(Rect::new(0, 0, 80, 100), 2, 25, 5);
    assert_eq!(uniform, vec![Rect::new(0, 0, 80, 25), Rect::new(0, 30, 80, 25)]);
}

#[test]
fn hit_test_skips_disabled_and_labels() {
    let mut ui = UiState::new(vec![
        Widget::Label(Label::new("title", Rect::new(0, 0, 100, 10))),
        Widget::Button(Button::new("off", Rect::new(0, 10, 100, 20)).with_enabled(false)),
        Widget::Button(Button::new("on", Rect::new(0, 30, 100, 20))),
        Widget::TextBox(TextBox::new("name", Rect::new(0, 50, 100, 20))),
    ]);

    assert_eq!(hit_test(&ui, Point::new(5, 5)), None, "метка не кликабельна");
    assert_eq!(hit_test(&ui, Point::new(5, 15)), None, "выключенная кнопка");
    assert_eq!(hit_test(&ui, Point::new(5, 35)), Some(2));
    assert_eq!(hit_test(&ui, Point::new(5, 55)), Some(3));

    ui.set_hover(Some(Point::new(5, 35)));
    assert_eq!(ui.hover, Some(2));
    ui.set_hover(Some(Point::new(500, 500)));
    assert_eq!(ui.hover, None);
}

#[test]
fn render_button_uses_theme_and_text() {
    let mut buffer = RgbaBuffer::filled(80, 30, Rgba::BLACK);
    let theme = UiTheme::default();
    let mut ui = UiState::new(vec![Widget::Button(Button::new(
            "OK",
        Rect::new(10, 5, 60, 20),
    ))]);
    ui.theme = theme;
    ui.focus = Some(0);

    render_ui(&mut buffer, &ui);

    assert_eq!(
        buffer.get(11, 6),
        Some(theme.button_hover),
            "фон кнопки в фокусе"
    );
    assert_eq!(buffer.get(10, 5), Some(theme.accent), "рамка фокуса");
    assert!(
        count_color(&buffer, theme.text) > 0,
            "подпись кнопки рисуется цветом темы"
    );
}

#[test]
fn render_disabled_button_is_grey_and_not_clickable() {
    let mut buffer = RgbaBuffer::new(80, 30);
    let theme = UiTheme::default();
    let button = Button::new("OFF", Rect::new(10, 5, 60, 20)).with_enabled(false);
    let ui = UiState::new(vec![Widget::Button(button)]);

    render_ui(&mut buffer, &ui);

    assert_eq!(buffer.get(11, 6), Some(theme.button_disabled));
    assert_eq!(buffer.get(10, 5), Some(theme.border));
    assert_eq!(ui.hit_test(Point::new(11, 6)), None);
}

#[test]
fn listbox_selection_scroll_and_rows() {
    let items = (0..20).map(|index| format!("game {index}")).collect();
    let mut list = ListBox::with_items(Rect::new(0, 0, 120, 42), items);

    assert_eq!(list.visible_rows(), 3);
    assert_eq!(list.selected, Some(0));
    assert_eq!(list.row_at(Point::new(5, 15)), Some(1));

    list.select(10);
    assert_eq!(list.selected, Some(10));
    assert_eq!(list.scroll, 8, "выбор прокручивает список");

    list.select_next(-100);
    assert_eq!(list.selected, Some(0));
    assert_eq!(list.scroll, 0);

    list.select_next(100);
    assert_eq!(list.selected, Some(19));

    let mut buffer = RgbaBuffer::new(120, 42);
    let ui = UiState::new(vec![Widget::ListBox(list)]);
    let theme = ui.theme;
    render_ui(&mut buffer, &ui);

    assert_eq!(
        buffer.get(5, 28),
        Some(theme.selection),
            "выбранная (последняя) строка подсвечена"
    );
}

#[test]
fn textbox_editing_and_render() {
    let mut text_box = TextBox::with_value("Name", "", "game", Rect::new(0, 0, 100, 20));
    assert!(!text_box.push_char('Ж'), "не-ASCII не принимается");
    assert!(text_box.push_char('M'));
    assert!(text_box.push_char('A'));
    assert_eq!(text_box.value(), "MA");
    assert!(text_box.backspace());
    assert_eq!(text_box.value(), "M");

    let mut buffer = RgbaBuffer::new(100, 20);
    let ui = UiState::new(vec![Widget::TextBox(text_box)]);
    render_ui(&mut buffer, &ui);

    assert_eq!(buffer.get(1, 1), Some(ui.theme.panel));
    assert!(count_color(&buffer, ui.theme.text) > 0);
}

#[test]
fn transparent_theme_keeps_the_underlying_frame() {
    let mut buffer = RgbaBuffer::filled(20, 10, Rgba::rgb(1, 2, 3));
    let mut ui = UiState::new(Vec::new());
    ui.theme.background = Rgba::TRANSPARENT;

    render_ui(&mut buffer, &ui);

    assert_eq!(buffer.get(0, 0), Some(Rgba::rgb(1, 2, 3)));
}

/// A stand-in for `max_render::hd_chrome::HdUiSkin`: fills the handled boxes
/// with one marker color. Proves that `render_ui_with_skin` asks the skin part
/// by part and keeps the procedural drawing for the rest.
struct MarkerSkin {
    color: Rgba,
    handled: &'static [UiBox],
}

impl UiBoxPainter for MarkerSkin {
    fn paint_box(&self, dst: &mut RgbaBuffer, rect: Rect, part: UiBox) -> bool {
        if !self.handled.contains(&part) {
            return false;
        }

        dst.fill_rect(rect.into(), self.color);
        true
    }
}

/// A screen that exercises every widget kind and every box slot.
fn mixed_screen() -> UiState {
    let mut list = ListBox::with_items(
        Rect::new(0, 60, 120, 42),
        vec!["one".to_string(), "two".to_string(), "three".to_string()],
    );
    list.select(0);

    UiState::new(vec![
        Widget::Label(Label::new("title", Rect::new(0, 0, 120, 12))),
        Widget::Button(Button::new("OK", Rect::new(0, 12, 120, 20))),
        Widget::Button(Button::new("OFF", Rect::new(0, 34, 120, 20)).with_enabled(false)),
        Widget::TextBox(TextBox::with_value(
            "Name",
            "game",
            "hint",
            Rect::new(0, 120, 120, 20),
        )),
        Widget::ListBox(list),
    ])
}

#[test]
fn a_skin_without_parts_is_byte_identical_to_render_ui() {
    let ui = mixed_screen();
    let mut plain = RgbaBuffer::new(160, 160);
    let mut skinned = RgbaBuffer::new(160, 160);

    render_ui(&mut plain, &ui);
    render_ui_with_skin(
        &mut skinned,
        &ui,
        Some(&MarkerSkin {
            color: Rgba::rgb(200, 100, 50),
            handled: &[],
        }),
    );

    assert_eq!(
        plain, skinned,
        "a skin that handles nothing changes nothing"
    );
}

#[test]
fn a_skin_replaces_only_the_boxes_it_handles_and_keeps_hit_testing() {
    let ui = mixed_screen();
    let widgets_before = ui.widgets.clone();
    let marker = Rgba::rgb(200, 100, 50);
    let mut buffer = RgbaBuffer::new(160, 160);

    render_ui_with_skin(
        &mut buffer,
        &ui,
        Some(&MarkerSkin {
            color: marker,
            handled: &[UiBox::Button],
        }),
    );

    // The normal button is the last-but-one "button" widget (index 1); its box
    // is painted by the skin, including where the procedural border was.
    assert_eq!(buffer.get(0, 12), Some(marker));
    assert_eq!(buffer.get(119, 31), Some(marker));
    // The disabled button (index 2) did not request `UiBox::Button` from the
    // fake skin and keeps the theme drawing.
    assert_eq!(buffer.get(1, 35), Some(ui.theme.button_disabled));
    assert_eq!(buffer.get(0, 35), Some(ui.theme.border));
    // The field and the list keep the theme panel; text is drawn either way.
    assert_eq!(buffer.get(1, 121), Some(ui.theme.panel));
    assert_eq!(buffer.get(5, 62), Some(ui.theme.selection));

    assert_eq!(ui.widgets, widgets_before, "geometry is read-only");
    for (point, expected) in [
        (Point::new(5, 20), Some(1)),
        (Point::new(5, 40), None),
        (Point::new(5, 130), Some(3)),
        (Point::new(5, 70), Some(4)),
    ] {
        assert_eq!(hit_test(&ui, point), expected, "{point:?}");
    }
}

#[test]
fn a_skin_can_paint_every_box_including_selection() {
    let ui = mixed_screen();
    let marker = Rgba::rgb(10, 200, 30);
    let mut buffer = RgbaBuffer::new(160, 160);

    render_ui_with_skin(
        &mut buffer,
        &ui,
        Some(&MarkerSkin {
            color: marker,
            handled: &UiBox::ALL,
        }),
    );

    assert_eq!(buffer.get(0, 0), Some(marker), "background skin");
    assert_eq!(buffer.get(1, 121), Some(marker), "field skin");
    assert_eq!(buffer.get(5, 62), Some(marker), "selection skin");
    assert_eq!(buffer.get(1, 35), Some(marker), "disabled button skin");
}

#[test]
fn a_transparent_theme_background_is_not_covered_by_the_skin() {
    let mut buffer = RgbaBuffer::filled(20, 10, Rgba::rgb(1, 2, 3));
    let mut ui = UiState::new(Vec::new());
    ui.theme.background = Rgba::TRANSPARENT;

    render_ui_with_skin(
        &mut buffer,
        &ui,
        Some(&MarkerSkin {
            color: Rgba::rgb(200, 100, 50),
            handled: &[UiBox::Background],
        }),
    );

    assert_eq!(
        buffer.get(0, 0),
        Some(Rgba::rgb(1, 2, 3)),
        "the transparent-background rule wins over the skin"
    );
}

#[test]
fn ui_box_slots_have_stable_manifest_names() {
    assert_eq!(
        UiBox::ALL.map(UiBox::name),
        [
            "background",
            "panel",
            "button",
            "button_hover",
            "button_disabled",
            "selection",
        ]
    );
}
