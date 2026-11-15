//! Кадр окна 1:1 с выбранным юнитом — доказательство пункта A.
//!
//! Грузит данные игры из репозитория, создаёт партию P2, выбирает первого
//! юнита игрока (чтобы заполнились портрет, статистика и описание) и пишет
//! кадр 640×480 в PPM. Дальше PPM → PNG любым конвертером
//! (`build/ppm2png.ps1` на Windows или Pillow).
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
        .state()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state().unit(id))
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
    // Панель координат показывает клетку под курсором, если мышь на карте.
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
