//! Headless frames of palette cycles and building connectors (session artifact).
//!
//! Builds the scene "mine + `POWGEN` adjacent" and saves five frames:
//! `dt = 0` (original palette, static body) and four phases of the palette
//! cycle 13–16 @6 fps — exactly what the DOS capture recorded in AVI
//! (`max-original-dasm/formats/unit-animation.md` §7.3: yellow → orange →
//! red → orange, 166.7 ms per phase, 666.7 ms full cycle).
//!
//! ```sh
//! cargo run -q -p max_app --no-default-features --example building_cycles -- qa-out/building-cycles
//! ```
//!
//! The default directory is `qa-out/building-cycles`. The files are PNG 640×480
//! (`qa::png`), plus the number of changed pixels between the phases and the
//! colors of the beacon pixels are printed to stdout.

use std::path::{Path, PathBuf};

use max_app::qa::png;
use max_app::AppConfig;
use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_core::TeamClan;
use max_render::buffer::RgbaBuffer;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn changed_pixels(left: &RgbaBuffer, right: &RgbaBuffer) -> usize {
    left.pixels()
        .chunks_exact(4)
        .zip(right.pixels().chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count()
}

fn save(dir: &Path, name: &str, frame: &RgbaBuffer) -> Result<(), String> {
    let image = png::Image::from_rgba(frame.width(), frame.height(), frame.pixels());
    let path = dir.join(format!("{name}.png"));

    png::write(&path, &image)?;
    println!("кадр {}", path.display());

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("qa-out/building-cycles"));
    std::fs::create_dir_all(&out)?;

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("assets"))
        .with_screen(640, 480);
    let mut app = max_app::App::new(config);
    app.init_assets()?;
    app.universe_mut().team_mut(TeamId::RED).expect("RED slot").team_type = TeamKind::Player;
    *app.universe_mut().team_units_mut(TeamId::RED).expect("RED economy") =
        max_core::TeamUnits::with_clan(TeamClan::Random);

    // Mine 2×2 at (30,22) and POWGEN 1×1 at (32,22): the mine has east
    // connectors, the generator has the west pipe (`D_SMLBLD` frame 5).
    let mine = app
        .universe_mut()
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(30, 22), 0)
        .expect("mining station");
    let powgen = app
        .universe_mut()
        .spawn_unit(UnitType::PowerGenerator, TeamId::RED, Point::new(32, 22), 0)
        .expect("power generator");

    for (id, orders) in [(mine, UnitOrder::PowerOn), (powgen, UnitOrder::PowerOn)] {
        let unit = app.universe_mut().unit_mut(id).expect("здание");
        unit.orders = orders;
        unit.state = UnitOrderState::ExecutingOrder;
    }
    app.viewport_mut().set_zoom(64);
    app.viewport_mut().center_on_grid(31, 23);

    // `dt = 0`: static C++ frame (`image_base + 1`), original palette.
    app.update_visual_units(0.0);
    let static_frame = app.render_frame();
    save(&out, "mine-phase-0-static", &static_frame)?;

    let mut previous = static_frame.clone();
    let mut frames = Vec::new();

    for phase in 1..=4 {
        // 6 fps step: 166.7 ms per phase; four steps — the full cycle.
        app.update_visual_units(1.0 / 6.0 + 0.001);
        let frame = app.render_frame();

        println!(
            "фаза {phase}: шагов 13..16 = {}, изменённых пикселей к предыдущей = {}",
            app.color_cycles().step_counts()[1],
            changed_pixels(&previous, &frame)
        );
        save(&out, &format!("mine-phase-{phase}"), &frame)?;
        previous = frame.clone();
        frames.push(frame);
    }

    println!(
        "полный цикл (фаза 4 → статичный кадр): изменённых пикселей = {}",
        changed_pixels(&frames[3], &static_frame)
    );

    // `dt = 0` returns exactly the static frame (goldens do not drift).
    app.update_visual_units(0.0);
    println!(
        "после dt = 0: изменённых пикселей к статичному кадру = {}",
        changed_pixels(&app.render_frame(), &static_frame)
    );

    Ok(())
}
