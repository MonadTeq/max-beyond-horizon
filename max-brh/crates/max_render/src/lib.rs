//! max_render — rendering of the M.A.X. world: palette, sprites, map tiles, camera.
//!
//! The crate is split into two parts:
//!
//! * **pure** — [`palette`], [`buffer`], [`sprite_atlas`], [`map_render`],
//!   [`viewport`], [`text`], [`ui`]: only data and integer arithmetic,
//!   no SDL and no file I/O (except loading `COLOR.PAL`). Everything is
//!   tested without a window — this is the main, always buildable layer.
//! * **SDL layer** — [`sdl_window`] behind the `sdl` feature: window, output of
//!   [`RgbaBuffer`] to the screen and the event queue. Without the feature the module does not build
//!   and the `sdl3` dependency is not pulled in.
//!
//! Correspondence with the original (`max-original-dasm/formats/spw.md`):
//!
//! * the `COLOR.PAL` palette — 256 × RGB at 6 bits, scaled to 8 bits by multiplying by 4 (§6.1);
//! * team slots 32–39 and 48–63 are recolored by the team LUT (§6.2);
//! * brightness — 7 levels plus identity, shadow — level 5 (§6.3);
//! * a sprite is drawn as `palette[brightness[team[index]]]`, transparent
//!   pixels are not written, the shadow darkens the already drawn map (§5.2, §4.3);
//! * sprite position — `anchor - hot` (§5.1), unit order — by `y` (§5.4).
//!
//! Running with a window (SDL3):
//!
//! ```text
//! # system SDL3 (dev package) — the usual path:
//! cargo run -p max_render --features sdl --example window
//! # without a system SDL3: build it from source (cmake required):
//! cargo run -p max_render --features sdl-from-source --example window
//! ```
//!
//! The `examples/window.rs` example draws a map and a sprite from `COLOR.PAL`/`TANK`
//! (if `max-original/MAX.RES` is next to the working directory).

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
    blit_simple_image, dot_radius_for, map_from_wrl, pattern_cell_size, render_map_into,
    render_tile, render_tile_into, tile_base_color, tile_pixel_color, TileStyle, PATTERN_CELLS,
    TILE_PIXELS,
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
        // Palette of 768 bytes + rendering of one pixel into the buffer without SDL.
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
