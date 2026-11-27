//! In-window menu bar of the SDL client: **File → Exit**, **Settings →
//! Resolutions → 640×480 (1:1) / 1280×960 (x2) / 2560×1920 (x4)**.
//!
//! The bar is drawn over the logical 640×480 frame (the same coordinate space
//! as the game UI and mouse hit tests), so it works in the main menu and in a
//! match without any change of the screen state machine:
//!
//! * [`WindowMenu::handle_input`] consumes only the clicks/keys that belong to
//!   an open or just-clicked menu; everything else is passed to the screen;
//! * [`WindowMenu::render_overlay`] draws the bar into a transparent overlay,
//!   and [`composite_overlay`] puts it on top of the final presentation frame
//!   (nearest `×S` in HD modes) — the game frame underneath is not modified;
//! * the only side effects are [`WindowMenuAction::Quit`] and
//!   [`WindowMenuAction::SetResolution`], which `main.rs` applies to the window
//!   presentation. Nothing here reads or writes the simulation.
//!
//! The bar is intentionally independent of [`crate::ui::Screen`]: a menu can be
//! opened in the main menu, a dialog or a running match, and a resolution change
//! keeps the same `App`/session (presentation only).
//!
//! Layout is in logical frame pixels; the longest label — `2560x1920 (x4)` — is
//! drawn with the embedded `FONT5` GNW font (10 px), so the menu needs no game
//! assets and stays readable at every scale.

use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::render_scale::RenderScale;
use max_render::text::{draw_text_gnw, GnwFontId};

use crate::ui::{InputEvent, Key};

/// Height of the menu bar in logical frame pixels (covers the `FONT5` cell).
pub const BAR_HEIGHT: i32 = 16;

/// Row height of a dropdown item in logical frame pixels.
pub const ROW_HEIGHT: i32 = 14;

/// Width of the resolutions flyout in logical frame pixels.
pub const FLYOUT_WIDTH: i32 = 156;

/// The three shipped resolutions, in menu order (`S = 1`, `2`, `4`).
pub const RESOLUTIONS: [RenderScale; 3] = [
    RenderScale::PixelPerfect,
    RenderScale::Hd2,
    RenderScale::Hd4,
];

/// Opaque bar background.
pub const BAR_BACKGROUND: Rgba = Rgba::rgb(18, 22, 30);
/// Bar bottom edge.
pub const BAR_BORDER: Rgba = Rgba::rgb(96, 126, 168);
/// Dropdown/flyout background.
pub const MENU_BACKGROUND: Rgba = Rgba::rgb(28, 34, 44);
/// Dropdown/flyout border.
pub const MENU_BORDER: Rgba = Rgba::rgb(128, 158, 200);
/// Label color.
pub const TEXT_COLOR: Rgba = Rgba::rgb(224, 232, 240);
/// Color of the current resolution marker.
pub const CURRENT_COLOR: Rgba = Rgba::rgb(246, 206, 92);
/// Hover/active background.
pub const HIGHLIGHT: Rgba = Rgba::rgb(52, 82, 128);

const TITLE_PADDING: i32 = 6;
const DROPDOWN_PADDING: i32 = 6;

/// Top-level menus of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuPage {
    /// `File`: the single item `Exit`.
    File,
    /// `Settings`: the `Resolutions` flyout.
    Settings,
}

impl MenuPage {
    /// All pages, left to right.
    pub const ALL: [Self; 2] = [Self::File, Self::Settings];

    /// Caption on the bar.
    pub const fn title(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Settings => "Settings",
        }
    }

    /// Dropdown width (fits the caption and the `>` marker).
    pub const fn dropdown_width(self) -> i32 {
        match self {
            Self::File => 96,
            Self::Settings => 124,
        }
    }

    /// Number of rows in the dropdown.
    pub const fn row_count(self) -> i32 {
        match self {
            Self::File => 1,
            Self::Settings => 1,
        }
    }
}

/// One selectable menu item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuItem {
    /// `File → Exit`: close the client.
    Exit,
    /// `Settings → Resolutions`: open the resolution flyout.
    Resolutions,
    /// One row of the flyout.
    Resolution(RenderScale),
}

/// What the driver must apply after an input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowMenuAction {
    /// The menu only changed its own state (opened/closed/flyout).
    #[default]
    None,
    /// `File → Exit`.
    Quit,
    /// A resolution was chosen: rebuild the window/render target at this scale.
    SetResolution(RenderScale),
}

/// Result of [`WindowMenu::handle_input`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MenuResponse {
    /// The driver action, if any.
    pub action: WindowMenuAction,
    /// The event belonged to the menu and must not reach the game screen.
    pub consumed: bool,
}

/// Hit region used for hover highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hover {
    Title(MenuPage),
    Item(MenuItem),
}

/// The in-window menu bar state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WindowMenu {
    open: Option<MenuPage>,
    flyout: bool,
    hover: Option<Hover>,
}

impl WindowMenu {
    /// A closed menu bar.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a dropdown is open (the menu then owns clicks and Esc).
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The open page, if any.
    pub fn open_page(&self) -> Option<MenuPage> {
        self.open
    }

    /// Whether the `Resolutions` flyout is open.
    pub fn is_flyout_open(&self) -> bool {
        self.flyout
    }

    /// Closes every dropdown, the flyout and the hover.
    pub fn close(&mut self) {
        self.open = None;
        self.flyout = false;
        self.hover = None;
    }

    /// Handles one UI event; returns the driver action and whether the event
    /// must be swallowed.
    pub fn handle_input(&mut self, event: InputEvent) -> MenuResponse {
        match event {
            InputEvent::MouseMove(x, y) => {
                self.update_hover(x, y);

                MenuResponse::default()
            }
            InputEvent::MouseClick(x, y) => self.click(x, y),
            // A swallowed click still emits a release; keep it out of the game so
            // a menu click never arms a button-press pulse behind the dropdown.
            InputEvent::MouseRelease(..) if self.is_open() => MenuResponse {
                action: WindowMenuAction::None,
                consumed: true,
            },
            // RMB closes the menu (the game uses it for cancel/inspect).
            InputEvent::RightClick(..) if self.is_open() => {
                self.close();

                MenuResponse {
                    action: WindowMenuAction::None,
                    consumed: true,
                }
            }
            InputEvent::Key(Key::Escape) if self.is_open() => {
                self.close();

                MenuResponse {
                    action: WindowMenuAction::None,
                    consumed: true,
                }
            }
            _ => MenuResponse::default(),
        }
    }

    /// Mouse move: bar titles and open dropdown rows are highlighted; hovering
    /// `Resolutions` opens its flyout.
    fn update_hover(&mut self, x: i32, y: i32) {
        self.hover = None;

        if let Some(page) = title_at(x, y) {
            self.hover = Some(Hover::Title(page));

            return;
        }

        let Some(page) = self.open else {
            return;
        };

        match page {
            MenuPage::File => {
                if item_rect(MenuItem::Exit).contains(x, y) {
                    self.hover = Some(Hover::Item(MenuItem::Exit));
                }
            }
            MenuPage::Settings => {
                if item_rect(MenuItem::Resolutions).contains(x, y) {
                    self.hover = Some(Hover::Item(MenuItem::Resolutions));
                    self.flyout = true;
                } else if let Some(scale) = resolution_at(x, y) {
                    if self.flyout {
                        self.hover = Some(Hover::Item(MenuItem::Resolution(scale)));
                    }
                }

                // The flyout stays open while the pointer is over the Settings
                // dropdown or the flyout itself; elsewhere it folds away.
                if self.flyout && !in_settings_menus(x, y) {
                    self.flyout = false;
                }
            }
        }
    }

    /// Mouse press on the bar/dropdowns.
    fn click(&mut self, x: i32, y: i32) -> MenuResponse {
        if let Some(page) = title_at(x, y) {
            if self.open == Some(page) {
                self.close();
            } else {
                self.open = Some(page);
                self.flyout = false;
                self.hover = Some(Hover::Title(page));
            }

            return MenuResponse {
                action: WindowMenuAction::None,
                consumed: true,
            };
        }

        let Some(page) = self.open else {
            return MenuResponse {
                action: WindowMenuAction::None,
                // The bar itself belongs to the window, not to the game screen.
                consumed: (0..BAR_HEIGHT).contains(&y),
            };
        };

        let action = match page {
            MenuPage::File => {
                if item_rect(MenuItem::Exit).contains(x, y) {
                    self.close();

                    WindowMenuAction::Quit
                } else {
                    WindowMenuAction::None
                }
            }
            MenuPage::Settings => {
                if item_rect(MenuItem::Resolutions).contains(x, y) {
                    self.flyout = true;

                    WindowMenuAction::None
                } else if self.flyout {
                    match resolution_at(x, y) {
                        Some(scale) => {
                            self.close();

                            WindowMenuAction::SetResolution(scale)
                        }
                        None => WindowMenuAction::None,
                    }
                } else {
                    WindowMenuAction::None
                }
            }
        };

        // A click inside the open dropdown keeps it open (an empty row area is
        // inert); a click outside dismisses the menu.
        if action == WindowMenuAction::None && !in_open_menu(page, x, y) {
            self.close();
        }

        MenuResponse {
            action,
            consumed: true,
        }
    }

    /// Draws the bar and the open dropdowns into a transparent `overlay`.
    ///
    /// The overlay keeps every pixel outside the menu transparent; the caller
    /// composites it over the final frame ([`composite_overlay`]). `current` is
    /// the running [`RenderScale`], marked with `*` in the flyout.
    pub fn render_overlay(&self, overlay: &mut RgbaBuffer, current: RenderScale) {
        overlay.fill(Rgba::TRANSPARENT);

        let width = overlay.width() as i32;

        if width <= 0 || (overlay.height() as i32) < BAR_HEIGHT {
            return;
        }

        overlay.fill_rect(PixelRect::new(0, 0, width, BAR_HEIGHT), BAR_BACKGROUND);
        overlay.fill_rect(PixelRect::new(0, BAR_HEIGHT - 1, width, 1), BAR_BORDER);

        for page in MenuPage::ALL {
            let rect = title_rect(page);

            if self.open == Some(page) {
                overlay.fill_rect(rect, HIGHLIGHT);
            } else if self.hover == Some(Hover::Title(page)) {
                overlay.fill_rect(rect, HIGHLIGHT.mix(BAR_BACKGROUND, 96));
            }

            draw_label(overlay, rect, page.title(), TEXT_COLOR);
        }

        let Some(page) = self.open else {
            return;
        };
        let dropdown = dropdown_rect(page);

        draw_panel(overlay, dropdown);

        match page {
            MenuPage::File => {
                let row = item_rect(MenuItem::Exit);

                if self.hover == Some(Hover::Item(MenuItem::Exit)) {
                    overlay.fill_rect(row, HIGHLIGHT);
                }

                draw_label(overlay, row, "Exit", TEXT_COLOR);
            }
            MenuPage::Settings => {
                let row = item_rect(MenuItem::Resolutions);

                if self.hover == Some(Hover::Item(MenuItem::Resolutions)) {
                    overlay.fill_rect(row, HIGHLIGHT);
                }

                draw_label(overlay, row, "Resolutions", TEXT_COLOR);

                // The submenu marker, right-aligned in the row.
                let arrow_width = GnwFontId::Font5.font().text_width(">");
                let arrow_rect = PixelRect::new(
                    row.lrx() - DROPDOWN_PADDING - arrow_width,
                    row.uly,
                    arrow_width,
                    row.height,
                );

                draw_label(overlay, arrow_rect, ">", TEXT_COLOR);

                if self.flyout {
                    self.render_flyout(overlay, current);
                }
            }
        }
    }

    /// Draws the `Resolutions` flyout under its parent row.
    fn render_flyout(&self, overlay: &mut RgbaBuffer, current: RenderScale) {
        let flyout = flyout_rect();

        draw_panel(overlay, flyout);

        for (index, scale) in RESOLUTIONS.into_iter().enumerate() {
            let row = resolution_row_rect(index);
            let selected = scale == current;

            if self.hover == Some(Hover::Item(MenuItem::Resolution(scale))) {
                overlay.fill_rect(row, HIGHLIGHT);
            }

            let label = format!(
                "{} {}",
                if selected { '*' } else { ' ' },
                resolution_label(scale)
            );
            let color = if selected { CURRENT_COLOR } else { TEXT_COLOR };

            draw_label(overlay, row, &label, color);
        }
    }
}

/// The caption of one resolution row (`640x480 (1:1)`, `1280x960 (x2)`, …).
pub fn resolution_label(scale: RenderScale) -> String {
    let (width, height) = scale.render_size();
    let tag = match scale {
        RenderScale::PixelPerfect => "1:1",
        RenderScale::Hd2 => "x2",
        RenderScale::Hd4 => "x4",
    };

    format!("{width}x{height} ({tag})")
}

/// Rect of a bar title in logical frame pixels.
pub fn title_rect(page: MenuPage) -> PixelRect {
    let font = GnwFontId::Font5.font();
    let mut x = 2;

    for candidate in MenuPage::ALL {
        let width = font.text_width(candidate.title()) + TITLE_PADDING * 2;

        if candidate == page {
            return PixelRect::new(x, 0, width, BAR_HEIGHT);
        }

        x += width;
    }

    PixelRect::new(2, 0, 1, BAR_HEIGHT)
}

/// Rect of the dropdown panel of `page`.
pub fn dropdown_rect(page: MenuPage) -> PixelRect {
    let title = title_rect(page);

    PixelRect::new(
        title.ulx,
        BAR_HEIGHT,
        page.dropdown_width(),
        page.row_count() * ROW_HEIGHT + 2,
    )
}

/// Rect of a dropdown item, in logical frame pixels.
pub fn item_rect(item: MenuItem) -> PixelRect {
    match item {
        MenuItem::Exit => {
            let dropdown = dropdown_rect(MenuPage::File);

            PixelRect::new(
                dropdown.ulx + 1,
                dropdown.uly + 1,
                dropdown.width - 2,
                ROW_HEIGHT,
            )
        }
        MenuItem::Resolutions => {
            let dropdown = dropdown_rect(MenuPage::Settings);

            PixelRect::new(
                dropdown.ulx + 1,
                dropdown.uly + 1,
                dropdown.width - 2,
                ROW_HEIGHT,
            )
        }
        MenuItem::Resolution(scale) => {
            let index = resolution_index(scale).unwrap_or(0);

            resolution_row_rect(index)
        }
    }
}

/// The flyout panel (to the right of the Settings dropdown).
pub fn flyout_rect() -> PixelRect {
    let dropdown = dropdown_rect(MenuPage::Settings);

    PixelRect::new(
        dropdown.lrx(),
        item_rect(MenuItem::Resolutions).uly,
        FLYOUT_WIDTH,
        RESOLUTIONS.len() as i32 * ROW_HEIGHT + 2,
    )
}

/// One row of the resolutions flyout.
fn resolution_row_rect(index: usize) -> PixelRect {
    let flyout = flyout_rect();

    PixelRect::new(
        flyout.ulx + 1,
        flyout.uly + 1 + index as i32 * ROW_HEIGHT,
        flyout.width - 2,
        ROW_HEIGHT,
    )
}

/// Index of `scale` in [`RESOLUTIONS`].
fn resolution_index(scale: RenderScale) -> Option<usize> {
    RESOLUTIONS
        .into_iter()
        .position(|candidate| candidate == scale)
}

/// Title under the point, if any.
fn title_at(x: i32, y: i32) -> Option<MenuPage> {
    if !(0..BAR_HEIGHT).contains(&y) {
        return None;
    }

    MenuPage::ALL
        .into_iter()
        .find(|page| title_rect(*page).contains(x, y))
}

/// Resolution row under the point (the flyout is only live while open).
fn resolution_at(x: i32, y: i32) -> Option<RenderScale> {
    RESOLUTIONS
        .into_iter()
        .find(|scale| item_rect(MenuItem::Resolution(*scale)).contains(x, y))
}

/// The point is inside the Settings dropdown or the resolutions flyout.
fn in_settings_menus(x: i32, y: i32) -> bool {
    dropdown_rect(MenuPage::Settings).contains(x, y) || flyout_rect().contains(x, y)
}

/// The point is inside the open page (dropdown, and the live flyout).
fn in_open_menu(page: MenuPage, x: i32, y: i32) -> bool {
    if dropdown_rect(page).contains(x, y) {
        return true;
    }

    page == MenuPage::Settings && flyout_rect().contains(x, y)
}

/// Draws a one-pixel-border panel.
fn draw_panel(buffer: &mut RgbaBuffer, rect: PixelRect) {
    buffer.fill_rect(rect, MENU_BACKGROUND);
    buffer.fill_rect(
        PixelRect::new(rect.ulx, rect.uly, rect.width, 1),
        MENU_BORDER,
    );
    buffer.fill_rect(
        PixelRect::new(rect.ulx, rect.lry() - 1, rect.width, 1),
        MENU_BORDER,
    );
    buffer.fill_rect(
        PixelRect::new(rect.ulx, rect.uly, 1, rect.height),
        MENU_BORDER,
    );
    buffer.fill_rect(
        PixelRect::new(rect.lrx() - 1, rect.uly, 1, rect.height),
        MENU_BORDER,
    );
}

/// Draws a label vertically centered in `rect`, left-aligned with padding.
fn draw_label(buffer: &mut RgbaBuffer, rect: PixelRect, text: &str, color: Rgba) {
    let font = GnwFontId::Font5.font();
    let y = rect.uly + (rect.height - font.height()).max(0) / 2;

    draw_text_gnw(buffer, rect.ulx + DROPDOWN_PADDING, y, font, text, color);
}

/// Composites the transparent menu overlay over `target` at the presentation
/// scale `scale` (nearest `×S`).
///
/// Only non-transparent overlay pixels are written: the game frame under the menu
/// is untouched, and in the HD modes the bar is a crisp `×S` upscale (the same
/// fallback rule as the other 1:1 menu screens, `remaster-hd.md` §R5).
pub fn composite_overlay(overlay: &RgbaBuffer, target: &mut RgbaBuffer, scale: u32) {
    let scale = scale.max(1);
    let overlay_width = overlay.width();

    if overlay_width == 0 || overlay.height() == 0 {
        return;
    }

    for (index, pixel) in overlay.pixels().chunks_exact(4).enumerate() {
        let alpha = pixel[3];

        if alpha == 0 {
            continue;
        }

        let x = (index as u32 % overlay_width) as i32;
        let y = (index as u32 / overlay_width) as i32;
        let color = Rgba::from_rgba([pixel[0], pixel[1], pixel[2], pixel[3]]);
        let rect = PixelRect::new(
            x * scale as i32,
            y * scale as i32,
            scale as i32,
            scale as i32,
        );

        if alpha == 255 {
            target.fill_rect(rect, color);
        } else {
            for dy in 0..scale as i32 {
                for dx in 0..scale as i32 {
                    target.modify(rect.ulx + dx, rect.uly + dy, |existing| {
                        // The frame itself stays opaque: only the RGB channels
                        // are blended, the target alpha is preserved.
                        let blended = existing.mix(color, alpha);

                        Rgba {
                            a: existing.a,
                            ..blended
                        }
                    });
                }
            }
        }
    }
}

/// Translates one window (presentation) event into the **game frame** space.
///
/// The window is [`BAR_HEIGHT`] logical pixels taller than the 640×480 game
/// frame: the bar sits above it. The game screen therefore receives the event
/// with `y` reduced by the bar height; keys are unchanged, and a pointer above
/// the frame becomes a negative `y` that no UI hit test matches.
pub fn to_game_event(input: InputEvent) -> InputEvent {
    match input {
        InputEvent::MouseClick(x, y) => InputEvent::MouseClick(x, y - BAR_HEIGHT),
        InputEvent::MouseRelease(x, y) => InputEvent::MouseRelease(x, y - BAR_HEIGHT),
        InputEvent::RightClick(x, y) => InputEvent::RightClick(x, y - BAR_HEIGHT),
        InputEvent::MouseMove(x, y) => InputEvent::MouseMove(x, y - BAR_HEIGHT),
        other => other,
    }
}

#[cfg(test)]
#[path = "window_menu_tests.rs"]
mod tests;
