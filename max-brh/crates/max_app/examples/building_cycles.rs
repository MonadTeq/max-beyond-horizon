//! Headless-кадры палитровых циклов и коннекторов зданий (артефакт сессии).
//!
//! Собирает сцену «шахта + `POWGEN` вплотную» и сохраняет пять кадров:
//! `dt = 0` (исходная палитра, статичный корпус) и четыре фазы палитрового
//! цикла 13–16 @6 fps — ровно то, что DOS-съёмка сняла в AVI
//! (`max-original-dasm/formats/unit-animation.md` §7.3: жёлтый → оранжевый →
//! красный → оранжевый, 166.7 мс на фазу, 666.7 мс полный цикл).
//!
//! ```sh
//! cargo run -q -p max_app --no-default-features --example building_cycles -- qa-out/building-cycles
//! ```
//!
//! Каталог по умолчанию — `qa-out/building-cycles`. Файлы — PNG 640×480
//! (`qa::png`), плюс в stdout печатается число изменившихся пикселей между
//! фазами и цвета пикселей-маяков.

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
    app.state_mut().teams[0].team_type = TeamKind::Player;
    app.state_mut().team_units[0] = max_core::TeamUnits::with_clan(TeamClan::Random);

    // Шахта 2×2 в (30,22) и POWGEN 1×1 в (32,22): у шахты восточные
    // коннекторы, у генератора — западная труба (`D_SMLBLD` кадр 5).
    let mine = app
        .state_mut()
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(30, 22), 0)
        .expect("mining station");
    let powgen = app
        .state_mut()
        .spawn_unit(UnitType::PowerGenerator, TeamId::RED, Point::new(32, 22), 0)
        .expect("power generator");

    for (id, orders) in [(mine, UnitOrder::PowerOn), (powgen, UnitOrder::PowerOn)] {
        let unit = app.state_mut().unit_mut(id).expect("здание");
        unit.orders = orders;
        unit.state = UnitOrderState::ExecutingOrder;
    }
    app.viewport_mut().set_zoom(64);
    app.viewport_mut().center_on_grid(31, 23);

    // `dt = 0`: статичный кадр C++ (`image_base + 1`), палитра исходная.
    app.update_visual_units(0.0);
    let static_frame = app.render_frame();
    save(&out, "mine-phase-0-static", &static_frame)?;

    let mut previous = static_frame.clone();
    let mut frames = Vec::new();

    for phase in 1..=4 {
        // Шаг 6 fps: 166.7 мс на фазу; четыре шага — полный цикл.
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

    // `dt = 0` возвращает ровно статичный кадр (golden не дрейфуют).
    app.update_visual_units(0.0);
    println!(
        "после dt = 0: изменённых пикселей к статичному кадру = {}",
        changed_pixels(&app.render_frame(), &static_frame)
    );

    Ok(())
}
