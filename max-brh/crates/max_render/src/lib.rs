//! max_render — rendering of the M.A.X. world: palette, sprites, map tiles, camera.
//!
//! The crate is split into two parts:
//!
//! * **pure** — [`palette`], [`buffer`], [`sprite_atlas`], [`map_render`],
//!   [`viewport`], [`text`], [`ui`], [`terrain`], [`hd_terrain`], [`hd_sprites`],
//!   [`hd_chrome`], [`render_scale`]: only data and integer arithmetic, no SDL.
//!   Everything is tested without a window — this is the main, always buildable
//!   layer.
//!   [`hd_terrain`] is the R3 presentation adapter: it reads decoded RGBA8
//!   textures through `max_assets::remaster` and never touches `max_core`.
//!   [`hd_sprites`] is the R4 adapter: HD index maps recolored on the CPU by a
//!   palette LUT (`ColorLut`/`BrightnessLut`/`ColorCycles`), with per-asset
//!   fallback to [`SpriteRenderer`].
//!   [`hd_chrome`] is the R5 adapter: pre-baked RGBA8 window chrome
//!   (`FRAMEPIC`/`PANELTOP`/`PANELBTM`, buttons and menus by `MAX.RES` name)
//!   and UI box skins, drawn at the presentation scale with per-asset fallback
//!   to the original palette images and the procedural [`ui`] theme.
//!   [`hd_text`] is the R6 adapter: HD **raster** GNW font atlases (no TTF),
//!   metrics and the `Text_TextLine` relief scaled exactly by `S`, with
//!   per-font fallback to the original GNW bitmap (nearest `×S`).
//!   [`gpu_aa`] is the pure part of the GPU anti-aliasing layer: the swapchain
//!   blit regions and the FXAA pass parameters (`gpu-aa-design.md`).
//! * **SDL layer** — [`sdl_window`] and [`gpu_present`] behind the `sdl` feature:
//!   window, output of [`RgbaBuffer`] to the screen (Canvas, or the GPU FXAA
//!   pass) and the event queue. Without the feature the modules do not build
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
pub mod gpu_aa;
pub mod hd_chrome;
pub mod hd_sprites;
pub mod hd_terrain;
pub mod hd_text;
pub mod layout;
pub mod map_render;
pub mod palette;
pub mod presentation;
pub mod render_scale;
pub mod sprite_atlas;
pub mod terrain;
pub mod text;
pub mod ui;
pub mod viewport;

#[cfg(feature = "sdl")]
pub mod gpu_present;

#[cfg(feature = "sdl")]
pub mod sdl_window;

pub use buffer::{PixelRect, Rgba, RgbaBuffer, BYTES_PER_PIXEL};
pub use frame_scale::{upscale_nearest, FrameScale, FRAME_HEIGHT, FRAME_WIDTH, MAX_SCALE};
pub use gpu_aa::{blit_regions, BlitRegions, FxaaParams, FXAA_LUMA_THRESHOLD};
pub use hd_chrome::{
    scaled_pixel_rect, scaled_pixel_rect_by, scaled_rect, scaled_rect_by, stretch_into,
    ChromePanel, ChromeSource, HdChrome, HdImage, HdUiSkin, CHROME_FRAME, CHROME_PANEL_BOTTOM,
    CHROME_PANEL_TOP, PANEL_BOTTOM_RECT, PANEL_TOP_RECT,
};
pub use hd_sprites::{
    is_silhouette_zoom, scaled_offset, HdIndexFrame, HdIndexMap, HdRecolorState, HdShadowFrame,
    HdSpriteBank, HdSpriteCache, HdSpriteKey, HdSpriteRenderer, HdSpriteTexture, IndexPaletteLut,
    COVERAGE_CHANNEL, DEFAULT_CACHE_LIMIT, PAYLOAD_CHANNEL, SILHOUETTE_ZOOM,
};
pub use hd_terrain::{HdTerrainAtlas, HdTile};
pub use hd_text::{
    draw_text_hd, draw_text_hd_outlined, draw_text_hd_outlined_scaled, draw_text_hd_scaled,
    draw_text_hd_shadowed, draw_text_line_hd, line_width_hd, HdFontAtlas, HdFonts, HdGlyphAtlas,
    FONT_COVERAGE_CHANNEL, GLYPH_COUNT, INK_THRESHOLD,
};
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
pub use presentation::{window_points_for, PresentationState, ScreenGeometry};
pub use render_scale::{
    AaMode, AaModeError, FilterMode, PresentationMode, PresentationModeError, RenderScale,
    HD_SCALES, HD_SCALE_X2, HD_SCALE_X4, PIXEL_PERFECT_SCALE,
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
    hit_test, layout, layout_uniform, render_ui, render_ui_with_skin, Button, Label, ListBox,
    Point, Rect, TextBox, UiBox, UiBoxPainter, UiState, UiTheme, Widget,
};
pub use viewport::{
    draw_order, draw_order_layered, GridLines, TileRange, Viewport, DEFAULT_ZOOM, MAP_TILE_PIXELS,
    MAX_ZOOM, MIN_ZOOM, ZOOM_STEP,
};

#[cfg(feature = "sdl")]
pub use gpu_present::{GpuPresentError, GpuPresenter};
#[cfg(feature = "sdl")]
pub use sdl_window::{SdlWindow, WindowError, WindowInput, WindowKey};

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
