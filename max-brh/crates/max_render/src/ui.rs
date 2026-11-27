//! Pure UI primitives for the client's screens: rectangles, buttons, input
//! fields, lists and vertical layout.
//!
//! There is neither SDL nor game logic here: [`render_ui`] draws [`UiState`] into
//! [`RgbaBuffer`], so any screen (menu, lobby, form) is tested by a
//! headless test — a buffer of the required size is enough.
//!
//! Terms:
//!
//! * [`Rect`] — a rectangle in screen pixels, the right/bottom edges are
//!   exclusive;
//! * [`Button`] — a button with a caption and an `enabled` flag; a disabled button
//!   is not included in [`hit_test`];
//! * [`TextBox`] — a single-line field (value + label + hint);
//! * [`ListBox`] — a list of strings with selection and scrolling;
//! * [`UiState`] — widgets + hover/focus + theme; [`render_ui`] draws the background,
//!   buttons, frames, selection and text.
//!
//! The font is the built-in 8×8 from [`crate::text`], so the UI does not depend on
//! `PATCHES.RES` and works before the game data is loaded.
//!
//! ```
//! use max_render::buffer::RgbaBuffer;
//! use max_render::ui::{layout, render_ui, Button, Rect, UiState, Widget};
//!
//! let mut frame = RgbaBuffer::new(120, 60);
//! let rects = layout(Rect::new(10, 10, 100, 40), &[20, 20], 4);
//! let ui = UiState::new(vec![
//!     Widget::Button(Button::new("Create", rects[0])),
//!     Widget::Button(Button::new("Quit", rects[1])),
//! ]);
//! render_ui(&mut frame, &ui);
//! ```

use crate::buffer::{PixelRect, Rgba, RgbaBuffer};
use crate::text::{draw_text_line, GLYPH_HEIGHT, GLYPH_WIDTH};

/// A point in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point {
    /// Coordinate along x.
    pub x: i32,
    /// Coordinate along y.
    pub y: i32,
}

impl Point {
    /// Point `(x, y)`.
    #[inline]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// A rectangle in screen pixels: top-left corner + size.
///
/// The right (`right`) and bottom (`bottom`) edges are exclusive — as with
/// [`PixelRect`], so neighboring rectangles do not overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width (may be 0).
    pub width: i32,
    /// Height (may be 0).
    pub height: i32,
}

impl Rect {
    /// Rectangle from a corner and a size.
    #[inline]
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Rectangle from two corners (in any order).
    #[inline]
    pub fn from_corners(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self {
            x: x0.min(x1),
            y: y0.min(y1),
            width: (x1 - x0).abs(),
            height: (y1 - y0).abs(),
        }
    }

    /// Right edge (exclusive).
    #[inline]
    pub const fn right(self) -> i32 {
        self.x + self.width
    }

    /// Bottom edge (exclusive).
    #[inline]
    pub const fn bottom(self) -> i32 {
        self.y + self.height
    }

    /// The area is zero or negative.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    /// The point is inside the rectangle (the edges are not included).
    #[inline]
    pub const fn contains(self, point: Point) -> bool {
        point.x >= self.x
            && point.y >= self.y
            && point.x < self.right()
            && point.y < self.bottom()
    }

    /// Center of the rectangle.
    #[inline]
    pub const fn center(self) -> Point {
        Point::new(self.x + self.width / 2, self.y + self.height / 2)
    }

    /// Shrinks the rectangle by `dx`/`dy` on each side.
    #[inline]
    pub fn inset(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            width: (self.width - dx * 2).max(0),
            height: (self.height - dy * 2).max(0),
        }
    }

    /// Intersection (empty — `None`).
    pub fn intersect(self, other: Self) -> Option<Self> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());

        (x0 < x1 && y0 < y1).then(|| Self::from_corners(x0, y0, x1, y1))
    }
}

impl From<Rect> for PixelRect {
    fn from(rect: Rect) -> Self {
        Self::new(rect.x, rect.y, rect.width, rect.height)
    }
}

/// A button with a caption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    /// Text on the button.
    pub label: String,
    /// Area of the button.
    pub rect: Rect,
    /// A disabled button cannot be pressed and is drawn gray.
    pub enabled: bool,
}

impl Button {
    /// Enabled button.
    pub fn new(label: impl Into<String>, rect: Rect) -> Self {
        Self {
            label: label.into(),
            rect,
            enabled: true,
        }
    }

    /// Disables/enables the button.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Whether the point hits the enabled button.
    pub fn hit_test(&self, point: Point) -> bool {
        self.enabled && self.rect.contains(point)
    }
}

/// Single-line input field.
///
/// The cursor is always at the end: that is enough for the UI skeleton, and keyboard
/// input reduces to [`TextBox::push_char`] / [`TextBox::backspace`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBox {
    /// Field label (drawn as `label: value`).
    pub label: String,
    /// Entered value.
    pub text: String,
    /// Hint when the value is empty.
    pub placeholder: String,
    /// Area of the field.
    pub rect: Rect,
    /// Maximum number of characters.
    pub max_chars: usize,
}

impl TextBox {
    /// Empty field with a label and an area.
    pub fn new(label: impl Into<String>, rect: Rect) -> Self {
        Self {
            label: label.into(),
            text: String::new(),
            placeholder: String::new(),
            rect,
            max_chars: 32,
        }
    }

    /// Field with a ready value and a hint.
    pub fn with_value(
        label: impl Into<String>,
        value: impl Into<String>,
        placeholder: impl Into<String>,
        rect: Rect,
    ) -> Self {
        Self {
            label: label.into(),
            text: value.into(),
            placeholder: placeholder.into(),
            rect,
            max_chars: 32,
        }
    }

    /// Limits the number of characters.
    pub fn with_max_chars(mut self, max_chars: usize) -> Self {
        self.max_chars = max_chars;
        self
    }

    /// Current value.
    pub fn value(&self) -> &str {
        &self.text
    }

    /// Sets the value (truncated by `max_chars`).
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.text = value.into().chars().take(self.max_chars).collect();
    }

    /// Appends a printable ASCII character; `false` if the character was not accepted.
    pub fn push_char(&mut self, ch: char) -> bool {
        if !(ch.is_ascii_graphic() || ch == ' ') {
            return false;
        }
        if self.text.chars().count() >= self.max_chars {
            return false;
        }

        self.text.push(ch);
        true
    }

    /// Removes the last character; `false` if the field is already empty.
    pub fn backspace(&mut self) -> bool {
        self.text.pop().is_some()
    }

    /// Clears the field.
    pub fn clear(&mut self) {
        self.text.clear();
    }

    /// Whether the point hits the field.
    pub fn hit_test(&self, point: Point) -> bool {
        self.rect.contains(point)
    }
}

/// A list of strings with selection and vertical scrolling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListBox {
    /// Lines of the list.
    pub items: Vec<String>,
    /// Area of the list.
    pub rect: Rect,
    /// Selected index (may be outside the visible part).
    pub selected: Option<usize>,
    /// First visible line.
    pub scroll: usize,
    /// Line height in pixels.
    pub row_height: i32,
}

impl ListBox {
    /// Empty list with a 14 px line.
    pub fn new(rect: Rect) -> Self {
        Self {
            items: Vec::new(),
            rect,
            selected: None,
            scroll: 0,
            row_height: 14,
        }
    }

    /// List with ready lines.
    pub fn with_items(rect: Rect, items: Vec<String>) -> Self {
        let mut list = Self::new(rect);
        list.items = items;
        list.selected = (!list.items.is_empty()).then_some(0);
        list
    }

    /// How many lines fit into the visible part.
    pub fn visible_rows(&self) -> usize {
        if self.row_height <= 0 || self.rect.height <= 0 {
            0
        } else {
            (self.rect.height / self.row_height) as usize
        }
    }

    /// Selects a line, scrolling to it.
    pub fn select(&mut self, index: usize) {
        if self.items.is_empty() {
            self.selected = None;
            self.scroll = 0;
            return;
        }

        let index = index.min(self.items.len() - 1);
        self.selected = Some(index);
        self.ensure_visible();
    }

    /// Moves the selection (clamped at the bounds, without wrapping around).
    pub fn select_next(&mut self, delta: isize) {
        if self.items.is_empty() {
            self.selected = None;
            return;
        }

        let current = self.selected.unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, self.items.len() as isize - 1) as usize;
        self.select(next);
    }

    /// Scrolls so that the selected line is visible.
    pub fn ensure_visible(&mut self) {
        let Some(selected) = self.selected else {
            self.scroll = 0;
            return;
        };
        let visible = self.visible_rows().max(1);

        if selected < self.scroll {
            self.scroll = selected;
        } else if selected >= self.scroll + visible {
            self.scroll = selected + 1 - visible;
        }

        self.scroll = self.scroll.min(self.items.len().saturating_sub(1));
    }

    /// The line under the point (taking scrolling into account).
    pub fn row_at(&self, point: Point) -> Option<usize> {
        if !self.rect.contains(point) || self.row_height <= 0 {
            return None;
        }

        let row = ((point.y - self.rect.y) / self.row_height).max(0) as usize;
        let index = self.scroll + row;

        (row < self.visible_rows() && index < self.items.len()).then_some(index)
    }

    /// Whether the point hits the list.
    pub fn hit_test(&self, point: Point) -> bool {
        self.rect.contains(point)
    }
}

/// Text label (heading, status, hint).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// Text.
    pub text: String,
    /// Area (the text is aligned to the left and centered vertically).
    pub rect: Rect,
    /// Custom color; `None` — the theme color [`UiTheme::text`].
    pub color: Option<Rgba>,
}

impl Label {
    /// Label with text and an area.
    pub fn new(text: impl Into<String>, rect: Rect) -> Self {
        Self {
            text: text.into(),
            rect,
            color: None,
        }
    }

    /// Label of an explicit color.
    pub fn colored(text: impl Into<String>, rect: Rect, color: Rgba) -> Self {
        Self {
            text: text.into(),
            rect,
            color: Some(color),
        }
    }
}

/// Screen widget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Widget {
    /// Button.
    Button(Button),
    /// Input field.
    TextBox(TextBox),
    /// List.
    ListBox(ListBox),
    /// Text label.
    Label(Label),
}

impl Widget {
    /// Area of the widget.
    pub fn rect(&self) -> Rect {
        match self {
            Self::Button(widget) => widget.rect,
            Self::TextBox(widget) => widget.rect,
            Self::ListBox(widget) => widget.rect,
            Self::Label(widget) => widget.rect,
        }
    }

    /// Changes the area of the widget (used by [`layout`]).
    pub fn set_rect(&mut self, rect: Rect) {
        match self {
            Self::Button(widget) => widget.rect = rect,
            Self::TextBox(widget) => widget.rect = rect,
            Self::ListBox(widget) => widget.rect = rect,
            Self::Label(widget) => widget.rect = rect,
        }
    }

    /// Whether the widget can receive focus/clicks.
    pub fn is_interactive(&self) -> bool {
        !matches!(self, Self::Label(_))
    }

    /// Whether the widget is enabled (only a button has `enabled`).
    pub fn is_enabled(&self) -> bool {
        match self {
            Self::Button(button) => button.enabled,
            _ => true,
        }
    }

    /// Whether the point hits the clickable part of the widget.
    pub fn hit_test(&self, point: Point) -> bool {
        match self {
            Self::Button(button) => button.hit_test(point),
            Self::TextBox(text_box) => text_box.hit_test(point),
            Self::ListBox(list) => list.hit_test(point),
            Self::Label(_) => false,
        }
    }
}

/// UI colors. A dark palette in the spirit of the original M.A.X.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiTheme {
    /// Screen background; `TRANSPARENT` — do not fill (e.g. over the map).
    pub background: Rgba,
    /// Background of fields and lists.
    pub panel: Rgba,
    /// Normal button.
    pub button: Rgba,
    /// Button under the cursor/in focus.
    pub button_hover: Rgba,
    /// Disabled button.
    pub button_disabled: Rgba,
    /// Normal frame.
    pub border: Rgba,
    /// Focus frame.
    pub accent: Rgba,
    /// Main text.
    pub text: Rgba,
    /// Text of a hint/disabled element.
    pub text_disabled: Rgba,
    /// Background of the selected list line.
    pub selection: Rgba,
    /// Text of the selected line.
    pub selection_text: Rgba,
}

impl Default for UiTheme {
    fn default() -> Self {
        Self {
            background: Rgba::rgb(12, 16, 24),
            panel: Rgba::rgb(22, 28, 40),
            button: Rgba::rgb(52, 64, 88),
            button_hover: Rgba::rgb(76, 96, 136),
            button_disabled: Rgba::rgb(38, 42, 50),
            border: Rgba::rgb(120, 140, 180),
            accent: Rgba::rgb(240, 196, 96),
            text: Rgba::rgb(232, 236, 244),
            text_disabled: Rgba::rgb(128, 132, 140),
            selection: Rgba::rgb(72, 112, 176),
            selection_text: Rgba::rgb(255, 255, 255),
        }
    }
}

/// UI state: widgets, hover, focus and theme.
///
/// `hover`/`focus` are indices into [`UiState::widgets`]. [`hit_test`] itself
/// skips labels and disabled buttons, so hover can be stored
/// as `mouse.and_then(|point| ui.hit_test(point))`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiState {
    /// Widgets in drawing order (the last one is on top for hit-test).
    pub widgets: Vec<Widget>,
    /// Index of the widget under the cursor.
    pub hover: Option<usize>,
    /// Index of the widget in focus.
    pub focus: Option<usize>,
    /// Colors.
    pub theme: UiTheme,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            widgets: Vec::new(),
            hover: None,
            focus: None,
            theme: UiTheme::default(),
        }
    }
}

impl UiState {
    /// UI with widgets and the default theme.
    pub fn new(widgets: Vec<Widget>) -> Self {
        Self {
            widgets,
            ..Self::default()
        }
    }

    /// UI with an explicit theme.
    pub fn with_theme(widgets: Vec<Widget>, theme: UiTheme) -> Self {
        Self {
            widgets,
            hover: None,
            focus: None,
            theme,
        }
    }

    /// Topmost clickable widget under the point.
    pub fn hit_test(&self, point: Point) -> Option<usize> {
        self.widgets
            .iter()
            .enumerate()
            .rev()
            .find(|(_, widget)| widget.hit_test(point))
            .map(|(index, _)| index)
    }

    /// Recomputes `hover` from the cursor position.
    pub fn set_hover(&mut self, mouse: Option<Point>) {
        self.hover = mouse.and_then(|point| self.hit_test(point));
    }

    /// Button by index.
    pub fn button(&self, index: usize) -> Option<&Button> {
        match self.widgets.get(index) {
            Some(Widget::Button(button)) => Some(button),
            _ => None,
        }
    }

    /// Mutable button by index.
    pub fn button_mut(&mut self, index: usize) -> Option<&mut Button> {
        match self.widgets.get_mut(index) {
            Some(Widget::Button(button)) => Some(button),
            _ => None,
        }
    }

    /// List by index.
    pub fn list_box(&self, index: usize) -> Option<&ListBox> {
        match self.widgets.get(index) {
            Some(Widget::ListBox(list)) => Some(list),
            _ => None,
        }
    }

    /// Mutable list by index.
    pub fn list_box_mut(&mut self, index: usize) -> Option<&mut ListBox> {
        match self.widgets.get_mut(index) {
            Some(Widget::ListBox(list)) => Some(list),
            _ => None,
        }
    }

    /// Input field by index.
    pub fn text_box(&self, index: usize) -> Option<&TextBox> {
        match self.widgets.get(index) {
            Some(Widget::TextBox(text_box)) => Some(text_box),
            _ => None,
        }
    }

    /// Mutable input field by index.
    pub fn text_box_mut(&mut self, index: usize) -> Option<&mut TextBox> {
        match self.widgets.get_mut(index) {
            Some(Widget::TextBox(text_box)) => Some(text_box),
            _ => None,
        }
    }
}

/// Topmost clickable widget under the point (free function).
pub fn hit_test(ui: &UiState, point: Point) -> Option<usize> {
    ui.hit_test(point)
}

/// Box part of a widget that a pre-baked skin can supply (`remaster-hd.md` §R5).
///
/// [`render_ui`] paints these parts procedurally from [`UiTheme`] (a fill and a
/// 1 px border). The HD remaster may substitute pre-baked RGBA8 art for any
/// part through [`UiBoxPainter`]; a part without a skin keeps the procedural
/// drawing, so the fallback is per part (guardrail 4 of the remaster plan).
///
/// The names are stable manifest slots (`max_assets::remaster::ui_skin_tag`),
/// not display strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UiBox {
    /// The themed screen background ([`UiTheme::background`]).
    Background,
    /// Background of input fields and lists (also inside the list box).
    Panel,
    /// A normal button.
    Button,
    /// A button under the cursor or in focus.
    ButtonHover,
    /// A disabled button.
    ButtonDisabled,
    /// The selected list row.
    Selection,
}

impl UiBox {
    /// Every slot, in drawing order (the order the remaster manifest lists them).
    pub const ALL: [Self; 6] = [
        Self::Background,
        Self::Panel,
        Self::Button,
        Self::ButtonHover,
        Self::ButtonDisabled,
        Self::Selection,
    ];

    /// Stable slot name used in the manifest tag (`button_hover`, ...).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Panel => "panel",
            Self::Button => "button",
            Self::ButtonHover => "button_hover",
            Self::ButtonDisabled => "button_disabled",
            Self::Selection => "selection",
        }
    }
}

/// A source of pre-baked widget box art (`remaster-hd.md` §R5).
///
/// Implemented by `max_render::hd_chrome::HdUiSkin`. [`render_ui_with_skin`]
/// asks the painter for each box; `false` means "no HD art for this part", and
/// the procedural theme drawing of [`render_ui`] is used instead.
///
/// A painter never changes widget geometry: the rectangles it receives are the
/// logical widget rectangles, so layout and [`UiState::hit_test`] are untouched.
pub trait UiBoxPainter {
    /// Paints the box `part` of `rect` into `dst`; `true` when it handled the
    /// box (the procedural fill and border are then skipped).
    fn paint_box(&self, dst: &mut RgbaBuffer, rect: Rect, part: UiBox) -> bool;
}

/// Vertical layout: rectangles top to bottom inside `area`.
///
/// `heights` are the element heights, `gap` is the spacing between them. Returns as many
/// rectangles as heights were passed; the width is the width of `area`.
pub fn layout(area: Rect, heights: &[i32], gap: i32) -> Vec<Rect> {
    let gap = gap.max(0);
    let mut y = area.y;

    heights
        .iter()
        .map(|&height| {
            let height = height.max(0);
            let rect = Rect::new(area.x, y, area.width, height);
            y += height + gap;
            rect
        })
        .collect()
}

/// Vertical layout of equal elements.
pub fn layout_uniform(area: Rect, count: usize, item_height: i32, gap: i32) -> Vec<Rect> {
    layout(area, &vec![item_height.max(0); count], gap)
}

impl UiState {
    /// Lays out all widgets vertically (changes their `rect`).
    pub fn layout_vertical(&mut self, area: Rect, item_height: i32, gap: i32) {
        let rects = layout_uniform(area, self.widgets.len(), item_height, gap);

        for (widget, rect) in self.widgets.iter_mut().zip(rects) {
            widget.set_rect(rect);
        }
    }
}

/// Draws the UI into the buffer.
///
/// First fills the background (if the theme is not transparent), then draws in order
/// the labels, fields, lists and buttons: background → frame → selection → text.
///
/// This is the pixel-perfect (no HD skin) path; [`render_ui_with_skin`] is the
/// same renderer with a source of pre-baked box art.
pub fn render_ui(buffer: &mut RgbaBuffer, ui: &UiState) {
    render_ui_with_skin(buffer, ui, None);
}

/// Draws the UI with an optional HD box skin (`remaster-hd.md` §R5).
///
/// The geometry is exactly that of [`render_ui`]: `ui` is read-only, widget
/// rectangles, layout and [`UiState::hit_test`] are never touched, and the
/// caller keeps the 640×480 logical coordinates. For each box the painter is
/// asked first; a part it does not handle is drawn procedurally by the theme,
/// so a partial HD skin (or `None`) renders every widget as before.
///
/// `skin` is `None` for the byte-for-byte 1:1 path; a skin that reports
/// "no HD" for every part (e.g. a `PixelPerfect` `HdUiSkin`) is byte-identical
/// to `None`.
pub fn render_ui_with_skin(buffer: &mut RgbaBuffer, ui: &UiState, skin: Option<&dyn UiBoxPainter>) {
    let theme = ui.theme;

    if !theme.background.is_transparent() {
        let bounds = Rect::new(0, 0, buffer.width() as i32, buffer.height() as i32);

        if !paint_box(buffer, skin, bounds, UiBox::Background) {
            buffer.fill(theme.background);
        }
    }

    for (index, widget) in ui.widgets.iter().enumerate() {
        let focused = ui.focus == Some(index);
        let hovered = ui.hover == Some(index);

        match widget {
            Widget::Label(label) => {
                let color = label.color.unwrap_or(theme.text);
                let max_chars = ((label.rect.width - 4).max(0) / GLYPH_WIDTH) as usize;
                let text = fit_text(&label.text, max_chars);
                let y = label.rect.y + (label.rect.height - GLYPH_HEIGHT) / 2;

                draw_text_line(buffer, label.rect.x + 2, y, &text, color);
            }
            Widget::Button(button) => {
                let fill = if !button.enabled {
                    theme.button_disabled
                } else if focused || hovered {
                    theme.button_hover
                } else {
                    theme.button
                };
                let border = if focused { theme.accent } else { theme.border };
                let part = if !button.enabled {
                    UiBox::ButtonDisabled
                } else if focused || hovered {
                    UiBox::ButtonHover
                } else {
                    UiBox::Button
                };

                if !paint_box(buffer, skin, button.rect, part) {
                    buffer.fill_rect(button.rect.into(), fill);
                    stroke_rect(buffer, button.rect, border);
                }

                let color = if button.enabled {
                    theme.text
                } else {
                    theme.text_disabled
                };
                let max_chars = ((button.rect.width - 8).max(0) / GLYPH_WIDTH) as usize;
                let label = fit_text(&button.label, max_chars);
                let text_width = crate::text::line_width(&label);
                let x = if text_width + 8 <= button.rect.width {
                    button.rect.x + (button.rect.width - text_width) / 2
                } else {
                    button.rect.x + 4
                };
                let y = button.rect.y + (button.rect.height - GLYPH_HEIGHT) / 2;

                draw_text_line(buffer, x, y, &label, color);
            }
            Widget::TextBox(text_box) => {
                let border = if focused { theme.accent } else { theme.border };

                if !paint_box(buffer, skin, text_box.rect, UiBox::Panel) {
                    buffer.fill_rect(text_box.rect.into(), theme.panel);
                    stroke_rect(buffer, text_box.rect, border);
                }

                let empty = text_box.text.is_empty();
                let value = if empty {
                    &text_box.placeholder
                } else {
                    &text_box.text
                };
                let mut line = if text_box.label.is_empty() {
                    value.clone()
                } else {
                    format!("{}: {}", text_box.label, value)
                };

                if focused {
                    line.push('_');
                }

                let max_chars = ((text_box.rect.width - 8).max(0) / GLYPH_WIDTH) as usize;
                let line = fit_text(&line, max_chars);
                let color = if empty {
                    theme.text_disabled
                } else {
                    theme.text
                };
                let y = text_box.rect.y + (text_box.rect.height - GLYPH_HEIGHT) / 2;

                draw_text_line(buffer, text_box.rect.x + 4, y, &line, color);
            }
            Widget::ListBox(list) => {
                let border = if focused || hovered {
                    theme.accent
                } else {
                    theme.border
                };

                if !paint_box(buffer, skin, list.rect, UiBox::Panel) {
                    buffer.fill_rect(list.rect.into(), theme.panel);
                    stroke_rect(buffer, list.rect, border);
                }

                if list.items.is_empty() {
                    draw_text_line(
                        buffer,
                        list.rect.x + 4,
                        list.rect.y + (list.row_height - GLYPH_HEIGHT) / 2,
                        "(empty)",
                        theme.text_disabled,
                    );
                    continue;
                }

                let row_height = list.row_height.max(GLYPH_HEIGHT);

                for row in 0..list.visible_rows() {
                    let index = list.scroll + row;
                    let Some(item) = list.items.get(index) else {
                        break;
                    };
                    let row_rect = Rect::new(
                        list.rect.x + 2,
                        list.rect.y + row as i32 * row_height,
                        list.rect.width - 4,
                        row_height,
                    );

                    if list.selected == Some(index)
                        && !paint_box(buffer, skin, row_rect, UiBox::Selection)
                    {
                        buffer.fill_rect(row_rect.into(), theme.selection);
                    }

                    let color = if list.selected == Some(index) {
                        theme.selection_text
                    } else {
                        theme.text
                    };
                    let max_chars = ((row_rect.width - 8).max(0) / GLYPH_WIDTH) as usize;
                    let text = fit_text(item, max_chars);
                    let y = row_rect.y + (row_rect.height - GLYPH_HEIGHT) / 2;

                    draw_text_line(buffer, row_rect.x + 4, y, &text, color);
                }
            }
        }
    }
}

/// Asks the skin to paint one box; `false` — the procedural theme drawing is used.
fn paint_box(
    buffer: &mut RgbaBuffer,
    skin: Option<&dyn UiBoxPainter>,
    rect: Rect,
    part: UiBox,
) -> bool {
    skin.is_some_and(|skin| skin.paint_box(buffer, rect, part))
}

/// Draws a 1 px frame along the edges of the rectangle.
fn stroke_rect(buffer: &mut RgbaBuffer, rect: Rect, color: Rgba) {
    if rect.is_empty() {
        return;
    }

    buffer.fill_rect(PixelRect::new(rect.x, rect.y, rect.width, 1), color);
    buffer.fill_rect(
        PixelRect::new(rect.x, rect.bottom() - 1, rect.width, 1),
        color,
    );
    buffer.fill_rect(PixelRect::new(rect.x, rect.y, 1, rect.height), color);
    buffer.fill_rect(
        PixelRect::new(rect.right() - 1, rect.y, 1, rect.height),
        color,
    );
}

/// Truncates the string to `max_chars` characters (the font is monospaced).
fn fit_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        text.chars().take(max_chars).collect()
    }
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
