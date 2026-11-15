//! max_render — рендер мира M.A.X.: палитра, спрайты, тайлы карты, камера.
//!
//! Крейт разделён на две части:
//!
//! * **чистая** — [`palette`], [`buffer`], [`sprite_atlas`], [`map_render`],
//!   [`viewport`], [`text`], [`ui`]: только данные и целочисленная арифметика,
//!   никакого SDL и файлового I/O (кроме загрузки `COLOR.PAL`). Всё
//!   тестируется без окна — это основной, всегда собираемый слой.
//! * **SDL-слой** — [`sdl_window`] под feature `sdl`: окно, вывод
//!   [`RgbaBuffer`] на экран и очередь событий. Без фичи модуль не собирается
//!   и зависимость `sdl3` не тянется.
//!
//! Соответствие оригиналу (`max-original-dasm/formats/spw.md`):
//!
//! * палитра `COLOR.PAL` — 256 × RGB по 6 бит, в 8 бит умножается на 4 (§6.1);
//! * командные слоты 32–39 и 48–63 перекрашиваются LUT команды (§6.2);
//! * яркость — 7 уровней плюс identity, тень — уровень 5 (§6.3);
//! * спрайт рисуется как `палитра[яркость[команда[индекс]]]`, прозрачные
//!   пиксели не пишутся, тень затемняет уже нарисованную карту (§5.2, §4.3);
//! * позиция спрайта — `anchor - hot` (§5.1), порядок юнитов — по `y` (§5.4).
//!
//! Запуск с окном (SDL3):
//!
//! ```text
//! # системный SDL3 (dev-пакет) — обычный путь:
//! cargo run -p max_render --features sdl --example window
//! # без системного SDL3: собрать его из исходников (нужен cmake):
//! cargo run -p max_render --features sdl-from-source --example window
//! ```
//!
//! Пример `examples/window.rs` рисует карту и спрайт из `COLOR.PAL`/`TANK`
//! (если рядом с рабочим каталогом есть `max-original/MAX.RES`).

#![forbid(unsafe_code)]

pub mod buffer;
pub mod frame_scale;
pub mod layout;
pub mod map_render;
pub mod palette;
pub mod sprite_atlas;
pub mod terrain;
pub mod text;
pub mod ui;
pub mod viewport;

#[cfg(feature = "sdl")]
pub mod sdl_window;

pub use buffer::{PixelRect, Rgba, RgbaBuffer, BYTES_PER_PIXEL};
pub use frame_scale::{upscale_nearest, FrameScale, FRAME_HEIGHT, FRAME_WIDTH, MAX_SCALE};
pub use layout::{
    AMMO_BUTTON, COLORS_BUTTON, CORNER_FLIC, END_TURN_BUTTON, FILES_BUTTON, GRID_BUTTON,
    HITS_BUTTON, MAIN_MAP, MINIMAP, NAME_BUTTON, PREFS_BUTTON, RANGE_BUTTON, SCAN_BUTTON,
    SIDE_PANEL_TOP_HEIGHT, SIDE_PANEL_WIDTH, STATUS_BUTTON, STAT_WINDOW, SURVEY_BUTTON,
    WINDOW_HEIGHT, WINDOW_WIDTH,
};
pub use map_render::{
    dot_radius_for, map_from_wrl, pattern_cell_size, render_map_into, render_tile,
    render_tile_into, tile_base_color, tile_pixel_color, TileStyle, PATTERN_CELLS, TILE_PIXELS,
};
pub use palette::{
    command_slots, map_color, BrightnessLut, ColorLut, Palette, PaletteError, TeamColors,
    BRIGHTNESS_LEVELS, COLOR_PAL_FILE_SIZE, COMMAND_SLOT_COUNT, FULL_BRIGHTNESS_LEVEL,
    PALETTE_BYTES, PALETTE_SIZE, SHADOW_BRIGHTNESS_LEVEL, TEAM_BLUE_COLORS, TEAM_DERELICT_COLORS,
    TEAM_GRAY_COLORS, TEAM_GREEN_COLORS, TEAM_RED_COLORS,
};
pub use sprite_atlas::{DrawStats, SpriteRenderer};
pub use terrain::{palette_from_wrl, PaletteRgba, TerrainAtlas};
pub use text::{
    draw_text, draw_text_gnw, draw_text_gnw_centered, draw_text_gnw_centered_shadowed,
    draw_text_gnw_shadowed, draw_text_line, draw_text_wrapped, glyph, line_width, text_height,
    wrap_text, GnwFont, GnwFontError, GnwFontId, GnwGlyph, TextMetrics, ASCII_CHARS,
    FALLBACK_GLYPH, FONT_8X8, GLYPH_HEIGHT, GLYPH_WIDTH,
};
pub use ui::{
    hit_test, layout, layout_uniform, render_ui, Button, Label, ListBox, Point, Rect, TextBox,
    UiState, UiTheme, Widget,
};
pub use viewport::{
    draw_order, TileRange, Viewport, DEFAULT_ZOOM, MAP_TILE_PIXELS, MAX_ZOOM, MIN_ZOOM, ZOOM_STEP,
};

#[cfg(feature = "sdl")]
pub use sdl_window::{SdlWindow, WindowError, WindowInput, WindowKey};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_render_smoke() {
        // Палитра из 768 байт + рендер одного пикселя в буфер без SDL.
        let bytes = [7u8; PALETTE_BYTES];
        let palette = Palette::from_color_pal(&bytes).unwrap();
        let brightness = BrightnessLut::build(&palette);
        let renderer = SpriteRenderer::neutral(&palette, &brightness);
        let mut buffer = RgbaBuffer::new(1, 1);

        assert!(buffer.set(0, 0, renderer.color_for(0)));
        assert_eq!(buffer.get(0, 0), Some(Rgba::rgb(28, 28, 28)));

        let viewport = Viewport::new(320, 240);

        assert_eq!(viewport.tile_pixels(), DEFAULT_ZOOM as i32);
        assert!(viewport.visible_tiles().is_some());
        assert_eq!(draw_order(&[(0, 1), (0, 0)]), vec![1, 0]);
    }
}
