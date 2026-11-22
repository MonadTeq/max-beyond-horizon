#![allow(dead_code)]

//! The editor's pure layer: selection/filter/zoom state and frame drawing.
//!
//! There is no SDL here: [`EditorState::compose`] draws the whole interface into
//! [`RgbaBuffer`], so the same code is used by the window (`app.rs`) and by the
//! headless snapshot (`--shot`). The list/field widgets come from `max_render::ui`.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use max_assets::ResArchive;
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::palette::Palette;
use max_render::text::{draw_text_line, GLYPH_HEIGHT, GLYPH_WIDTH};
use max_render::ui::{Button, ListBox, Point, Rect, TextBox, UiState, UiTheme, Widget};

use crate::model::{classify_archive, Category, Kind, ResourceInfo};
use crate::png::RgbaImage;
use crate::render::render_resource;

/// The logical frame width.
pub const FRAME_WIDTH: u32 = 1280;
/// The logical frame height.
pub const FRAME_HEIGHT: u32 = 800;
/// The window title.
pub const WINDOW_TITLE: &str = "M.A.X. - MAX.RES resource editor";

const HEADER_HEIGHT: i32 = 56;
const LIST_RECT: Rect = Rect::new(10, HEADER_HEIGHT, 408, 704);
const PREVIEW_RECT: Rect = Rect::new(430, HEADER_HEIGHT, 840, 568);
const META_RECT: Rect = Rect::new(430, 632, 840, 156);
const FOOTER_Y: i32 = 790;

const BG: Rgba = Rgba::rgb(14, 18, 26);
const PANEL: Rgba = Rgba::rgb(24, 30, 42);
const BORDER: Rgba = Rgba::rgb(92, 112, 146);
const TEXT: Rgba = Rgba::rgb(228, 233, 242);
const DIM: Rgba = Rgba::rgb(150, 160, 178);
const ACCENT: Rgba = Rgba::rgb(240, 196, 96);
const CHECKER_A: Rgba = Rgba::rgb(200, 204, 210);
const CHECKER_B: Rgba = Rgba::rgb(232, 235, 240);

/// The editor state.
pub struct EditorState {
    /// The archive (needed to re-decode the selected resource).
    pub archive: ResArchive,
    /// The classified records.
    pub infos: Vec<ResourceInfo>,
    /// The system COLOR.PAL palette.
    pub palette: Palette,
    /// The filter string.
    pub filter: String,
    /// Focus in the search field.
    pub search_focus: bool,
    /// The list (items — captions of the filtered rows).
    pub list: ListBox,
    /// The `infos` indices corresponding to the list rows.
    pub rows: Vec<usize>,
    /// The preview scale (1..=8).
    pub zoom: i32,
    /// The preview offset in screen pixels.
    pub pan: (i32, i32),
    /// The cache of the rasterized selected resource.
    preview: Option<(usize, RgbaImage)>,
    /// The last mouse position in frame coordinates.
    pub mouse: Point,
    /// A preview drag is in progress.
    pub dragging: bool,
    /// The last drag point.
    drag_last: Point,
    /// The message at the bottom (errors/screenshot saving).
    pub status: String,
}

impl EditorState {
    /// Classifies the archive and selects `select` (or the first image).
    pub fn new(archive: ResArchive, palette: Palette, select: Option<&str>) -> Self {
        let infos = classify_archive(&archive);
        let mut state = Self {
            archive,
            infos,
            palette,
            filter: String::new(),
            search_focus: false,
            list: ListBox::new(LIST_RECT),
            rows: Vec::new(),
            zoom: 1,
            pan: (0, 0),
            preview: None,
            mouse: Point::new(-1, -1),
            dragging: false,
            drag_last: Point::new(0, 0),
            status: String::new(),
        };
        state.list.row_height = 14;
        state.refresh_rows();

        let target = select
            .and_then(|name| state.infos.iter().position(|info| info.name == name))
            .or_else(|| state.infos.iter().position(|info| info.kind.is_image()));

        if let Some(entry) = target {
            state.select_entry(entry);
        }

        state
    }

    /// Full archive statistics.
    pub fn stats(&self) -> crate::model::ClassificationStats {
        crate::model::ClassificationStats::from_infos(&self.infos)
    }

    /// Rebuilds the list rows by the filter (substring, case-insensitive).
    pub fn refresh_rows(&mut self) {
        let previous = self.selected_info().map(|info| info.name.clone());
        let needle = self.filter.to_ascii_uppercase();

        self.rows = self
            .infos
            .iter()
            .enumerate()
            .filter(|(_, info)| {
                needle.is_empty() || info.name.to_ascii_uppercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect();
        self.list.items = self
            .rows
            .iter()
            .map(|&index| self.infos[index].list_label())
            .collect();

        let selected_row = previous
            .and_then(|name| {
                self.rows
                    .iter()
                    .position(|&index| self.infos[index].name == name)
            })
            .or_else(|| (!self.rows.is_empty()).then_some(0));

        self.list.selected = selected_row;
        self.list.scroll = selected_row
            .unwrap_or(0)
            .saturating_sub(self.list.visible_rows() / 2);
        self.list.ensure_visible();
        self.preview = None;
        self.zoom = 1;
        self.pan = (0, 0);
    }

    /// The current record.
    pub fn selected_info(&self) -> Option<&ResourceInfo> {
        let row = self.list.selected?;
        let index = *self.rows.get(row)?;

        self.infos.get(index)
    }

    /// Selects the record by its index in `infos`.
    pub fn select_entry(&mut self, entry_index: usize) {
        if let Some(row) = self.rows.iter().position(|&index| index == entry_index) {
            self.list.select(row);
            self.preview = None;
            self.zoom = 1;
            self.pan = (0, 0);
        }
    }

    /// Shift the selection (Up/Down).
    pub fn select_delta(&mut self, delta: isize) {
        self.list.select_next(delta);
        self.preview = None;
        self.zoom = 1;
        self.pan = (0, 0);
    }

    /// Shift the selection by a page (PgUp/PgDn).
    pub fn select_page(&mut self, direction: isize) {
        let page = self.list.visible_rows().max(1) as isize;
        self.select_delta(direction * page);
    }

    /// Home/End.
    pub fn select_edge(&mut self, end: bool) {
        if self.rows.is_empty() {
            return;
        }
        self.list.select(if end { self.rows.len() - 1 } else { 0 });
        self.preview = None;
        self.zoom = 1;
        self.pan = (0, 0);
    }

    /// Text into the filter (printable ASCII). Automatically enables the search.
    pub fn push_filter_char(&mut self, ch: char) {
        if !(ch.is_ascii_graphic() || ch == ' ') {
            return;
        }

        self.search_focus = true;
        self.filter.push(ch);
        self.refresh_rows();
    }

    /// Backspace in the filter.
    pub fn filter_backspace(&mut self) {
        if self.filter.pop().is_some() {
            self.refresh_rows();
        }
    }

    /// F3 — focus/unfocus the search.
    pub fn toggle_search(&mut self) {
        self.search_focus = !self.search_focus;
    }

    /// Enter — apply the filter and return the focus to the list.
    pub fn apply_filter(&mut self) {
        self.search_focus = false;
    }

    /// Clear the filter (Esc in the search field).
    pub fn clear_filter(&mut self) {
        if !self.filter.is_empty() {
            self.filter.clear();
            self.refresh_rows();
        }
    }

    /// Zoom in/out.
    pub fn zoom_by(&mut self, delta: i32) {
        self.zoom = (self.zoom + delta).clamp(1, 8);
    }

    /// Fit the selected image into the preview window.
    pub fn fit_zoom(&mut self) {
        let Some((width, height)) = self.preview_size() else {
            return;
        };
        let view = PREVIEW_RECT.inset(4, 4);

        if width == 0 || height == 0 {
            return;
        }

        let zoom_x = view.width / width as i32;
        let zoom_y = view.height / height as i32;
        self.zoom = zoom_x.min(zoom_y).clamp(1, 8);
        self.pan = (0, 0);
    }

    /// Reset the zoom/pan.
    pub fn reset_view(&mut self) {
        self.zoom = 1;
        self.pan = (0, 0);
    }

    /// Shift the preview (wheel/dragging).
    pub fn pan_by(&mut self, dx: i32, dy: i32) {
        self.pan.0 += dx;
        self.pan.1 += dy;
        self.clamp_pan();
    }

    /// A mouse click: the list, the filter field, the start of a preview drag.
    pub fn click(&mut self, x: i32, y: i32) {
        let point = Point::new(x, y);

        if self.filter_rect().contains(point) {
            self.search_focus = true;
            self.status.clear();
            return;
        }
        if self.list.rect.contains(point) {
            self.search_focus = false;
            if let Some(row) = self.list.row_at(point) {
                self.list.select(row);
                self.preview = None;
                self.zoom = 1;
                self.pan = (0, 0);
            }
            return;
        }
        if PREVIEW_RECT.contains(point) && self.has_preview() {
            self.dragging = true;
            self.drag_last = point;
        }
    }

    /// Mouse release.
    pub fn release(&mut self) {
        self.dragging = false;
    }

    /// Mouse movement.
    pub fn mouse_move(&mut self, x: i32, y: i32) {
        let point = Point::new(x, y);
        self.mouse = point;

        if self.dragging {
            self.pan_by(point.x - self.drag_last.x, point.y - self.drag_last.y);
            self.drag_last = point;
        }
    }

    /// The wheel: scrolling the preview or the list.
    pub fn wheel(&mut self, x: f32, y: f32, ctrl: bool) {
        if ctrl {
            self.zoom_by(if y > 0.0 { 1 } else { -1 });
            return;
        }

        if self.has_preview() && PREVIEW_RECT.contains(self.mouse) {
            let dx = if x.abs() > y.abs() {
                -(x as i32) * 32
            } else {
                0
            };
            let dy = if x.abs() > y.abs() {
                0
            } else {
                -(y as i32) * 32
            };
            self.pan_by(dx, dy);
        } else if self.list.rect.contains(self.mouse) {
            let step = if y > 0.0 { -3 } else { 3 };
            self.select_delta(step);
        }
    }

    /// The size of the selected image (strip/image), without decoding.
    pub fn preview_size(&self) -> Option<(u32, u32)> {
        let info = self.selected_info()?;
        if !info.kind.is_image() {
            return None;
        }

        match info.kind {
            Kind::Simple | Kind::Big => Some((info.width.max(0) as u32, info.height.max(0) as u32)),
            Kind::Sprite | Kind::Shadow | Kind::Cursor => {
                let frames: Vec<_> = info
                    .frames
                    .iter()
                    .map(|frame| (frame.width, frame.height, frame.hot_x, frame.hot_y))
                    .collect();

                Some(crate::render::strip_dimensions(&frames))
            }
            Kind::NonImage => None,
        }
    }

    /// The cached RGBA of the selected image (after `compose`).
    pub fn preview_image(&self) -> Option<&RgbaImage> {
        self.preview.as_ref().map(|(_, image)| image)
    }

    /// The selected resource has a preview.
    pub fn has_preview(&self) -> bool {
        self.selected_info()
            .map(|info| info.kind.is_image())
            .unwrap_or(false)
    }

    fn ensure_preview(&mut self) -> Result<()> {
        let Some(info) = self.selected_info() else {
            self.preview = None;
            return Ok(());
        };
        if !info.kind.is_image() {
            self.preview = None;
            return Ok(());
        }

        let index = info.index;
        if self.preview.as_ref().map(|(cached, _)| *cached) != Some(index) {
            let image = render_resource(info, &self.archive, &self.palette)
                .with_context(|| format!("превью {}", info.name))?;
            self.preview = Some((index, image));
        }

        Ok(())
    }

    fn clamp_pan(&mut self) {
        let Some((width, height)) = self.preview_size() else {
            self.pan = (0, 0);
            return;
        };
        let view = PREVIEW_RECT.inset(4, 4);
        let scaled_w = width as i32 * self.zoom;
        let scaled_h = height as i32 * self.zoom;
        let margin = 24;

        let min_x = (view.x + margin) - scaled_w;
        let max_x = view.right() - margin;
        let min_y = (view.y + margin) - scaled_h;
        let max_y = view.bottom() - margin;

        self.pan.0 = self.pan.0.clamp(min_x.min(max_x), max_x.max(min_x));
        self.pan.1 = self.pan.1.clamp(min_y.min(max_y), max_y.max(min_y));
    }

    fn filter_rect(&self) -> Rect {
        Rect::new(380, 12, 420, 26)
    }

    /// The complete interface frame.
    pub fn compose(&mut self) -> RgbaBuffer {
        let _ = self.ensure_preview();

        let mut frame = RgbaBuffer::filled(FRAME_WIDTH, FRAME_HEIGHT, BG);
        let stats = self.stats();

        // The title.
        draw_text_line(&mut frame, 12, 8, "M.A.X.  MAX.RES resource editor", ACCENT);
        draw_text_line(
            &mut frame,
            12,
            30,
            &format!(
                "{} entries | {} images | {} non-image",
                stats.entries, stats.images, stats.non_images
            ),
            DIM,
        );
        draw_text_line(
            &mut frame,
            820,
            10,
            &format!("shown {}/{}", self.rows.len(), self.infos.len()),
            TEXT,
        );
        draw_text_line(
            &mut frame,
            820,
            28,
            &format!(
                "selected {}",
                self.selected_info()
                    .map(|info| info.name.as_str())
                    .unwrap_or("-")
            ),
            DIM,
        );

        // The filter field and the list — with max_render::ui widgets.
        let filter = TextBox::with_value(
            "Filter",
            self.filter.clone(),
            "substring (F3)",
            self.filter_rect(),
        )
        .with_max_chars(40);
        let mut ui = UiState::new(vec![
            Widget::TextBox(filter),
            Widget::ListBox(self.list.clone()),
        ]);
        let mut theme = UiTheme::default();
        theme.background = Rgba::TRANSPARENT;
        theme.panel = PANEL;
        theme.border = BORDER;
        theme.text = TEXT;
        theme.text_disabled = DIM;
        theme.accent = ACCENT;
        theme.selection = Rgba::rgb(62, 96, 150);
        theme.selection_text = Rgba::rgb(255, 255, 255);
        ui.theme = theme;

        if self.search_focus {
            ui.focus = Some(0);
        } else if self.list.rect.contains(self.mouse) {
            ui.hover = Some(1);
        }

        max_render::ui::render_ui(&mut frame, &ui);

        self.draw_scrollbar(&mut frame);

        // The preview.
        frame.fill_rect(PREVIEW_RECT.into(), PANEL);
        stroke_rect(&mut frame, PREVIEW_RECT, BORDER);
        let view = PREVIEW_RECT.inset(4, 4);
        checkerboard(
            &mut frame,
            Rect::new(view.x, view.y + 22, view.width, view.height - 22),
        );
        self.draw_preview(&mut frame);

        // Metadata.
        frame.fill_rect(META_RECT.into(), PANEL);
        stroke_rect(&mut frame, META_RECT, BORDER);
        self.draw_metadata(&mut frame);

        // The hint/status.
        let hint = "Up/Down select | PgUp/PgDn page | +/- zoom | 0 reset | F5 fit | F3 search | wheel/drag pan | F12 screenshot | Esc quit";
        draw_text_line(
            &mut frame,
            12,
            FOOTER_Y - 4,
            &fit(hint, FRAME_WIDTH as i32 - 24),
            DIM,
        );
        if !self.status.is_empty() {
            draw_text_line(
                &mut frame,
                12,
                FOOTER_Y - 16,
                &fit(&self.status, FRAME_WIDTH as i32 - 24),
                ACCENT,
            );
        }

        frame
    }

    fn draw_scrollbar(&self, frame: &mut RgbaBuffer) {
        if self.rows.len() <= self.list.visible_rows() {
            return;
        }

        let track = Rect::new(
            self.list.rect.right() - 9,
            self.list.rect.y + 2,
            7,
            self.list.rect.height - 4,
        );
        frame.fill_rect(track.into(), Rgba::rgb(38, 46, 60));

        let visible = self.list.visible_rows().max(1);
        let thumb_height = ((track.height as f32 * visible as f32 / self.rows.len() as f32).round()
            as i32)
            .clamp(12, track.height);
        let max_scroll = self.rows.len().saturating_sub(visible).max(1) as f32;
        let offset = (track.height - thumb_height) as f32 * self.list.scroll as f32 / max_scroll;
        let thumb = Rect::new(
            track.x + 1,
            track.y + offset.round() as i32,
            track.width - 2,
            thumb_height,
        );
        frame.fill_rect(thumb.into(), Rgba::rgb(132, 154, 190));
    }

    fn draw_preview(&mut self, frame: &mut RgbaBuffer) {
        let Some(info) = self.selected_info().cloned() else {
            draw_text_line(
                frame,
                PREVIEW_RECT.x + 12,
                PREVIEW_RECT.y + 12,
                "no selection",
                DIM,
            );
            return;
        };

        let title = if info.kind.is_image() {
            format!(
                "{} | {} | {}x{} | {} frames | zoom {}x",
                info.name,
                info.kind.as_str(),
                info.width,
                info.height,
                info.frame_count,
                self.zoom
            )
        } else {
            format!(
                "{} | non-image | {}",
                info.name,
                info.non_image.map(|kind| kind.as_str()).unwrap_or("?")
            )
        };

        if !info.kind.is_image() {
            draw_text_line(
                frame,
                PREVIEW_RECT.x + 16,
                PREVIEW_RECT.y + 40,
                "NON-IMAGE - preview disabled",
                DIM,
            );
            draw_text_line(
                frame,
                PREVIEW_RECT.x + 16,
                PREVIEW_RECT.y + 58,
                &format!(
                    "type: {}   {}",
                    info.non_image.map(|kind| kind.as_str()).unwrap_or("?"),
                    info.file_ref.as_deref().unwrap_or("")
                ),
                TEXT,
            );
            draw_text_line(
                frame,
                PREVIEW_RECT.x + 16,
                PREVIEW_RECT.y + 76,
                &format!("first bytes: {}", info.first_bytes_hex()),
                TEXT,
            );
            draw_text_line(
                frame,
                PREVIEW_RECT.x + 8,
                PREVIEW_RECT.y + 6,
                &fit(&title, PREVIEW_RECT.width - 16),
                ACCENT,
            );
            return;
        }

        let Some((_, image)) = self.preview.as_ref() else {
            return;
        };
        let image_width = image.width as i32;
        let image_height = image.height as i32;
        if image_width == 0 || image_height == 0 {
            return;
        }

        let view = PREVIEW_RECT.inset(4, 4);
        let view_top = view.y + 22;
        let scaled_width = image_width * self.zoom;
        let scaled_height = image_height * self.zoom;
        let base_x = if scaled_width > view.width {
            view.x + self.pan.0
        } else {
            view.x + (view.width - scaled_width) / 2 + self.pan.0
        };
        let base_y = if scaled_height > view.height - 22 {
            view_top + self.pan.1
        } else {
            view_top + (view.height - 22 - scaled_height) / 2 + self.pan.1
        };

        let clip = Rect::new(
            base_x.max(view.x),
            base_y.max(view_top),
            (base_x + scaled_width).min(view.right()) - base_x.max(view.x),
            (base_y + scaled_height).min(view.bottom()) - base_y.max(view_top),
        );
        let clip = Rect::new(
            clip.x,
            clip.y,
            clip.width.min(view.right() - clip.x),
            clip.height.min(view.bottom() - clip.y),
        );

        if clip.is_empty() {
            return;
        }

        for screen_y in clip.y..clip.bottom() {
            for screen_x in clip.x..clip.right() {
                let source_x = (screen_x - base_x) / self.zoom;
                let source_y = (screen_y - base_y) / self.zoom;
                if source_x < 0
                    || source_y < 0
                    || source_x >= image_width
                    || source_y >= image_height
                {
                    continue;
                }
                let Some(pixel) = image.pixel(source_x as u32, source_y as u32) else {
                    continue;
                };
                if pixel[3] == 0 {
                    continue;
                }
                let destination = frame.get(screen_x, screen_y).unwrap_or(Rgba::BLACK);
                let blended = if pixel[3] == 255 {
                    Rgba::rgb(pixel[0], pixel[1], pixel[2])
                } else {
                    blend(destination, pixel)
                };
                frame.set(screen_x, screen_y, blended);
            }
        }

        // A border around the image.
        let bounds = Rect::new(base_x, base_y, scaled_width, scaled_height);
        if bounds.right() > view.x && bounds.x < view.right() {
            stroke_rect(frame, bounds, Rgba::rgb(70, 84, 104));
        }

        // The title is drawn last so that the frames do not cover it.
        draw_text_line(
            frame,
            PREVIEW_RECT.x + 8,
            PREVIEW_RECT.y + 6,
            &fit(&title, PREVIEW_RECT.width - 16),
            ACCENT,
        );
    }

    fn draw_metadata(&self, frame: &mut RgbaBuffer) {
        let Some(info) = self.selected_info() else {
            return;
        };

        let mut lines: Vec<(String, Rgba)> = Vec::new();
        lines.push((
            format!(
                "{}   #{} / {}   kind: {}   category: {}",
                info.name,
                info.index + 1,
                self.infos.len(),
                info.kind.as_str(),
                info.category.as_str()
            ),
            TEXT,
        ));
        lines.push((
            format!(
                "offset 0x{:08X}   size {} bytes   frame {}x{}   frame_count {}   palette {}",
                info.offset,
                info.size,
                info.width,
                info.height,
                info.frame_count,
                info.palette.as_str()
            ),
            TEXT,
        ));
        lines.push((
            format!(
                "ulx/uly {}/{}   transparent {}   frame sizes {}",
                info.ulx,
                info.uly,
                info.transparent
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                info.frame_size_range()
            ),
            DIM,
        ));

        if let Some(kind) = info.non_image {
            lines.push((
                format!(
                    "non-image type: {}   ref: {}",
                    kind.as_str(),
                    info.file_ref.as_deref().unwrap_or("-")
                ),
                ACCENT,
            ));
            lines.push((
                format!(
                    "first bytes: {}   (offset 0x{:08X})",
                    info.first_bytes_hex(),
                    info.offset
                ),
                TEXT,
            ));
        } else {
            lines.push((
                format!(
                    "strip/export: {}   category {}",
                    info.file_name(),
                    info.category.dir().unwrap_or("-")
                ),
                ACCENT,
            ));
            if info.frame_count > 1 {
                lines.push((
                    format!(
                        "frames: {} in a horizontal strip, aligned by hot spot",
                        info.frame_count
                    ),
                    DIM,
                ));
            }
        }

        for (index, (line, color)) in lines.iter().enumerate() {
            let y = META_RECT.y + 8 + index as i32 * 12;
            if y + GLYPH_HEIGHT > META_RECT.bottom() {
                break;
            }
            draw_text_line(
                frame,
                META_RECT.x + 8,
                y,
                &fit(line, META_RECT.width - 16),
                *color,
            );
        }
    }

    /// Saves the current frame to PNG (F12).
    pub fn save_screenshot(&mut self, path: &Path) -> Result<()> {
        let frame = self.compose();
        let image = RgbaImage {
            width: frame.width(),
            height: frame.height(),
            rgba: frame.pixels().to_vec(),
        };
        crate::png::write_rgba(path, &image)
            .map_err(|error| anyhow!("скриншот {}: {error}", path.display()))?;
        self.status = format!("screenshot saved: {}", path.display());

        Ok(())
    }

    /// The category of the selection (for the window title).
    pub fn selected_title(&self) -> String {
        match self.selected_info() {
            Some(info) => format!("{} — {} {}", WINDOW_TITLE, info.name, info.kind.as_str()),
            None => WINDOW_TITLE.to_string(),
        }
    }
}

fn blend(destination: Rgba, source: [u8; 4]) -> Rgba {
    let alpha = u32::from(source[3]);
    let rest = 255 - alpha;
    let mix = |dst: u8, src: u8| ((u32::from(dst) * rest + u32::from(src) * alpha) / 255) as u8;

    Rgba::rgb(
        mix(destination.r, source[0]),
        mix(destination.g, source[1]),
        mix(destination.b, source[2]),
    )
}

fn checkerboard(frame: &mut RgbaBuffer, rect: Rect) {
    for y in rect.y..rect.bottom() {
        for x in rect.x..rect.right() {
            let color = if ((x / 8) + (y / 8)) % 2 == 0 {
                CHECKER_A
            } else {
                CHECKER_B
            };
            frame.set(x, y, color);
        }
    }
}

fn stroke_rect(frame: &mut RgbaBuffer, rect: Rect, color: Rgba) {
    if rect.is_empty() {
        return;
    }
    frame.fill_rect(PixelRect::new(rect.x, rect.y, rect.width, 1), color);
    frame.fill_rect(
        PixelRect::new(rect.x, rect.bottom() - 1, rect.width, 1),
        color,
    );
    frame.fill_rect(PixelRect::new(rect.x, rect.y, 1, rect.height), color);
    frame.fill_rect(
        PixelRect::new(rect.right() - 1, rect.y, 1, rect.height),
        color,
    );
}

fn fit(text: &str, width: i32) -> String {
    let max_chars = (width.max(0) / GLYPH_WIDTH) as usize;
    let count = text.chars().count();

    if count <= max_chars {
        text.to_string()
    } else if max_chars == 0 {
        String::new()
    } else {
        format!(
            "{}...",
            text.chars()
                .take(max_chars.saturating_sub(3))
                .collect::<String>()
        )
    }
}

/// The list of favorite tags for the tests/screenshot.
pub fn find_info<'a>(infos: &'a [ResourceInfo], name: &str) -> Option<&'a ResourceInfo> {
    infos.iter().find(|info| info.name == name)
}

/// A check that the record is an image of the `category` category.
pub fn first_in_category(infos: &[ResourceInfo], category: Category) -> Option<&ResourceInfo> {
    infos
        .iter()
        .find(|info| info.kind.is_image() && info.category == category)
}

/// Zoom buttons (for drawing; clicks are handled in app).
pub fn zoom_buttons() -> [Button; 2] {
    [
        Button::new(
            "+",
            Rect::new(PREVIEW_RECT.right() - 66, PREVIEW_RECT.y + 4, 28, 18),
        ),
        Button::new(
            "-",
            Rect::new(PREVIEW_RECT.right() - 34, PREVIEW_RECT.y + 4, 28, 18),
        ),
    ]
}

/// An extra check: the record is not an image.
pub fn is_non_image(info: &ResourceInfo) -> bool {
    info.kind == Kind::NonImage
}

#[cfg(test)]
mod tests {
    use super::*;
    use max_assets::ResArchive;
    use max_render::palette::Palette;

    fn workspace_path(relative: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

    fn state() -> Option<EditorState> {
        let max_res = workspace_path("max-original/MAX.RES");
        let pal = workspace_path("max-original/COLOR.PAL");
        if !max_res.is_file() || !pal.is_file() {
            return None;
        }
        let archive = ResArchive::open(max_res).ok()?;
        let palette = Palette::load(pal).ok()?;

        Some(EditorState::new(archive, palette, Some("TANK")))
    }

    #[test]
    fn filter_is_case_insensitive_and_rebuilds_rows() {
        let Some(mut state) = state() else {
            return;
        };
        let all = state.rows.len();

        state.filter = "tank".to_string();
        state.refresh_rows();

        assert!(state.rows.len() < all);
        assert!(state.rows.iter().all(|&index| state.infos[index]
            .name
            .to_ascii_uppercase()
            .contains("TANK")));
        assert_eq!(
            state.selected_info().map(|info| info.name.as_str()),
            Some("TANK")
        );
    }

    #[test]
    fn selection_moves_and_pages() {
        let Some(mut state) = state() else {
            return;
        };
        let first = state.selected_info().unwrap().name.clone();
        state.select_delta(1);
        assert_ne!(state.selected_info().unwrap().name, first);

        state.select_page(1);
        assert!(state.list.selected.unwrap() >= state.list.visible_rows());

        state.select_edge(true);
        assert_eq!(state.list.selected, Some(state.rows.len() - 1));
    }

    #[test]
    fn compose_draws_the_selected_sprite() {
        let Some(mut state) = state() else {
            return;
        };
        let frame = state.compose();

        assert_eq!(frame.width(), FRAME_WIDTH);
        assert_eq!(frame.height(), FRAME_HEIGHT);
        assert!(frame.pixels().chunks_exact(4).any(|pixel| pixel[3] == 255));
    }
}
