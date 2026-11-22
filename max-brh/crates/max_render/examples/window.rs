//! `max_render` demo window: a map with a tank on it (SDL3).
//!
//! Run (SDL3 required — see `src/sdl_window.rs`):
//!
//! ```text
//! cargo run -p max_render --features sdl --example window
//! cargo run -p max_render --features sdl-from-source --example window
//! ```
//!
//! The demo needs no game data: the map is built procedurally, and the tank sprite
//! is picked up from `max-original/MAX.RES` (+ `COLOR.PAL`) if the files are
//! next to the working directory. Otherwise a colored unit marker is drawn.
//!
//! Controls: Esc or closing the window — exit.

use std::path::Path;

use max_assets::sprite::{decode_shadow, decode_sprite, ShadowSheet, SpriteSheet};
use max_assets::ResArchive;
use max_core::enums::Surface;
use max_core::map::{Map, ResourceKind, ResourceMap, ResourceMapEntry, SurfaceMap};
use max_render::buffer::{Rgba, RgbaBuffer};
use max_render::map_render::{render_map_into, TileStyle};
use max_render::palette::{BrightnessLut, ColorLut, Palette, TeamColors, FULL_BRIGHTNESS_LEVEL};
use max_render::sprite_atlas::SpriteRenderer;
use max_render::viewport::Viewport;
use max_render::SdlWindow;

const SCREEN_WIDTH: u32 = 512;
const SCREEN_HEIGHT: u32 = 384;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut window = SdlWindow::new("M.A.X. — max_render (R2.3)", SCREEN_WIDTH, SCREEN_HEIGHT)?;
    let style = TileStyle::default();
    let map = demo_map();
    let viewport = Viewport::new(SCREEN_WIDTH, SCREEN_HEIGHT);
    let mut frame = RgbaBuffer::filled(SCREEN_WIDTH, SCREEN_HEIGHT, style.unknown);

    // Game data is optional: without it the demo still shows the map.
    let game = GameAssets::load().ok();

    println!(
        "окно {}×{}, карта {}×{}; Esc — выход",
        window.size().0,
        window.size().1,
        map.size.width,
        map.size.height
    );

    loop {
        render_map_into(&mut frame, &map, &viewport, &style);

        let anchor = (SCREEN_WIDTH as i32 / 2, SCREEN_HEIGHT as i32 / 2);

        match &game {
            Some(assets) => {
                let renderer = SpriteRenderer::new(
                    &assets.palette,
                    &assets.brightness,
                    ColorLut::for_team(TeamColors::Red),
                    FULL_BRIGHTNESS_LEVEL,
                );

                if let Some(sprite) = assets.sprite.as_ref().and_then(|sheet| sheet.frame(0)) {
                    let shadow = assets.shadow.as_ref().and_then(|sheet| sheet.frame(0));

                    renderer.draw_unit(&mut frame, sprite, shadow, anchor, (6, 6));
                }
            }
            None => {
                // Unit placeholder: a red 12×12 diamond.
                for offset in 0..12 {
                    frame.set(anchor.0 - 6 + offset, anchor.1 - 6, Rgba::rgb(220, 40, 40));
                    frame.set(anchor.0 - 6, anchor.1 - 6 + offset, Rgba::rgb(220, 40, 40));
                    frame.set(anchor.0 + 6 - offset, anchor.1 + 6, Rgba::rgb(220, 40, 40));
                    frame.set(anchor.0 + 6, anchor.1 + 6 - offset, Rgba::rgb(220, 40, 40));
                }
            }
        }

        window.present(&frame)?;

        if !window.poll() {
            break;
        }
    }

    Ok(())
}

/// Palette, brightness tables and (if present) sprites from the game data.
struct GameAssets {
    palette: Palette,
    brightness: BrightnessLut,
    sprite: Option<SpriteSheet>,
    shadow: Option<ShadowSheet>,
}

impl GameAssets {
    fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let palette = Palette::load(Path::new("max-original/COLOR.PAL"))?;
        let brightness = BrightnessLut::build(&palette);
        let archive = ResArchive::open(Path::new("max-original/MAX.RES"))?;
        let sprite = archive
            .read("TANK")
            .ok()
            .and_then(|bytes| decode_sprite(bytes).ok());
        let shadow = archive
            .read("S_TANK")
            .ok()
            .and_then(|bytes| decode_shadow(bytes).ok());

        println!(
            "данные игры: COLOR.PAL + MAX.RES (TANK: {}, S_TANK: {})",
            sprite.is_some(),
            shadow.is_some()
        );

        Ok(Self {
            palette,
            brightness,
            sprite,
            shadow,
        })
    }
}

/// Procedural map: an island of land in water, a bit of coastline and resources.
fn demo_map() -> Map {
    let mut surface = SurfaceMap::new();
    let mut resources = ResourceMap::new();
    let centre = (56i16, 56i16);

    for y in 0..max_core::geo::MAP_HEIGHT {
        for x in 0..max_core::geo::MAP_WIDTH {
            let dx = i32::from(x - centre.0);
            let dy = i32::from(y - centre.1);
            let distance = dx * dx + dy * dy;

            let tile = if distance < 16 * 16 {
                Surface::LAND
            } else if distance < 22 * 22 {
                Surface::COAST
            } else {
                Surface::WATER
            };

            surface.set(x, y, tile);
        }
    }

    // Resources: a deterministic "pseudo-random" grid.
    let mut seed = 0x1234_5678u32;

    for _ in 0..24 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let x = 56 + ((seed >> 16) % 17) as i16 - 8;
        let y = 56 + ((seed >> 8) % 17) as i16 - 8;
        let kind = match (seed >> 24) % 3 {
            0 => ResourceKind::Raw,
            1 => ResourceKind::Fuel,
            _ => ResourceKind::Gold,
        };
        let amount = 1 + ((seed >> 4) % 8) as u8;

        resources.set(x, y, ResourceMapEntry::new(amount, Some(kind), 0x0F));
    }

    Map::from_parts(surface, resources)
}
