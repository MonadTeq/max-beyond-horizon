//! 1:1 window frame with a selected unit — proof of item A.
//!
//! Loads the game data from the repository, creates a P2 game, selects the first
//! player unit (so that the portrait, stats and description are filled) and writes
//! a 640×480 frame to PPM. Then PPM → PNG with any converter
//! (`build/ppm2png.ps1` on Windows or Pillow).
//!
//! ```sh
//! cargo run -p max_app --example panel_frame -- ../build/frame-panels.ppm
//! ```

use std::path::{Path, PathBuf};

use max_app::render::write_ppm;
use max_app::{App, AppConfig, NewGameConfig};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn main() -> anyhow::Result<()> {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("frame-panels.ppm"));

    let config = AppConfig {
        max_res: repo_path("max-original/MAX.RES"),
        color_pal: repo_path("max-original/COLOR.PAL"),
        patches_res: repo_path("assets/PATCHES.RES"),
        ..Default::default()
    };
    let mut app = App::new(config);
    let report = app.init_assets()?;

    println!(
        "assets: MAX.RES={} панели={} спрайтов={}",
        report.max_res_entries,
        app.assets().is_some_and(|assets| assets.panels.is_some()),
        report.sprite_count()
    );

    app.new_game(&NewGameConfig::p2_reference())?;

    let unit = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state_ref().unit(id))
        .find(|unit| {
            unit.team == app.player_team()
                && unit.hits > 0
                && app
                    .assets()
                    .is_some_and(|assets| assets.sprite(unit.unit_type).is_some())
        })
        .map(|unit| (unit.id, unit.grid, unit.name.clone()))
        .expect("в партии есть стартовый юнит игрока со спрайтом");

    app.hud_mut().select(Some(unit.0));
    app.viewport_mut().center_on_grid(unit.1.x, unit.1.y);
    // The coordinate panel shows the cell under the cursor when the mouse is on the map.
    app.hud_mut().panels.cursor_grid = Some((unit.1.x, unit.1.y));

    let (frame, stats) = app.render_frame_with_stats();

    write_ppm(&out, &frame)?;

    println!(
        "frame: {}x{} tiles={} sprites={} markers={} selected={:?} grid=({}, {})",
        frame.width(),
        frame.height(),
        stats.map_tiles,
        stats.units_drawn,
        stats.units_marker,
        unit.2,
        unit.1.x,
        unit.1.y
    );
    println!("ppm: {}", out.display());

    Ok(())
}
