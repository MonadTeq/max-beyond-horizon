//! 1:1 game window chrome: `FRAMEPIC` (640×480 background with its own palette) and the halves
//! of the left panel `PANELTOP`/`PANELBTM` (171×239/241, system palette).
//!
//! The original (`ResourceManager_InitInGameAssets`) does
//! `WindowManager_LoadBigImage(FRAMEPIC, window, …, palette_from_image=true)` —
//! that is, the whole in-game UI (panels, buttons, frames) is already drawn in
//! `FRAMEPIC`, and the map is rendered into the [`crate::layout::MAIN_MAP`] rectangle.
//! This is reproduced here: [`Chrome::draw`] fills the background, [`blit_map`]
//! puts the map into the map area.

use max_assets::image::{BigImage, SimpleImage};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::layout;

/// Loaded window chrome.
#[derive(Debug, Clone)]
pub struct Chrome {
    /// 640×480 window background (own palette, RLE).
    pub frame: BigImage,
    /// Top half of the left panel (for reporting/checks).
    pub panel_top: Option<SimpleImage>,
    /// Bottom half of the left panel.
    pub panel_bottom: Option<SimpleImage>,
}

impl Chrome {
    /// Assembles the chrome from the `MAX.RES` archive.
    ///
    /// `FRAMEPIC` is mandatory (without it there is no 1:1 window), the panels are optional:
    /// they are needed only for the landing screens; in-game everything is already in `FRAMEPIC`.
    pub fn load(max_res: &max_assets::ResArchive) -> Option<Self> {
        let frame = max_res
            .read("FRAMEPIC")
            .ok()
            .and_then(|bytes| BigImage::decode(bytes).ok())?;

        let read_simple = |name: &str| {
            max_res
                .read(name)
                .ok()
                .and_then(|bytes| SimpleImage::decode(bytes).ok())
        };

        Some(Self {
            frame,
            panel_top: read_simple("PANELTOP"),
            panel_bottom: read_simple("PANELBTM"),
        })
    }

    /// `true` if the window background is suitable for 1:1 (640×480).
    pub fn is_ready(&self) -> bool {
        self.frame.width as i32 == layout::WINDOW_WIDTH
            && self.frame.height as i32 == layout::WINDOW_HEIGHT
    }

    /// Fills `dst` with the window background (the palette is taken from the picture itself).
    ///
    /// Returns the number of filled pixels.
    pub fn draw(&self, dst: &mut RgbaBuffer) -> u32 {
        if !self.is_ready() {
            return 0;
        }

        let mut painted = 0;

        for y in 0..self.frame.height as u32 {
            for x in 0..self.frame.width as u32 {
                let Some([r, g, b]) = self.frame.rgb(x, y) else {
                    continue;
                };

                if dst.set(x as i32, y as i32, Rgba::rgb(r, g, b)) {
                    painted += 1;
                }
            }
        }

        painted
    }

    /// Background pixel (for tests/diagnostics).
    pub fn frame_rgb(&self, x: u32, y: u32) -> Option<[u8; 3]> {
        self.frame.rgb(x, y)
    }
}

/// Copies the map buffer into the [`layout::MAIN_MAP`] area of the window.
///
/// The map is drawn into its own 448×448 buffer (7×7 cells), so
/// it is clipped automatically by the area edges — like `WINDOW_MAIN_MAP` in
/// the original. Returns the number of copied pixels.
pub fn blit_map(dst: &mut RgbaBuffer, map: &RgbaBuffer) -> u32 {
    let area = layout::MAIN_MAP;
    let mut painted = 0;

    for y in 0..area.height.min(map.height() as i32) {
        for x in 0..area.width.min(map.width() as i32) {
            let Some(color) = map.get(x, y) else {
                continue;
            };

            if dst.set(area.ulx + x, area.uly + y, color) {
                painted += 1;
            }
        }
    }

    painted
}

/// Map area rectangle (for translating mouse coordinates).
pub fn map_area() -> PixelRect {
    layout::MAIN_MAP
}

#[cfg(test)]
#[path = "chrome_tests.rs"]
mod tests;
