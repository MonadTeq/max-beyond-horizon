//! Headless artifact TC-0933: the unit sprite always faces the direction of movement.
//!
//! Runs the L-shaped route of the `Scout` (no turret — the body frame reads
//! unambiguously): 2 cells east, then 2 north, in the same window loop
//! as `main.rs` (`tick` + `update_visual_units(dt)`), and captures
//! 640×480 frames with chrome/panels:
//!
//! * `01-east-with-sim-ne.png` — the simulation has already started turning north
//!   (at the cell boundary, `angle=1`, NE), while the sprite still moves east and faces
//!   east (`visual_angle=2`);
//! * `02-turn.png` — the sprite reached the turn cell and turned north;
//! * `03-stopped-north.png` — the route is complete, the angle equals the last direction.
//!
//! Checks (failure — exit 1): while the sprite moves along an axis, `visual_angle`
//! equals the direction of movement; after stopping `visual_angle == unit.angle`.
//!
//! ```sh
//! cargo run -p max_app --no-default-features --example angle_demo -- qa-out/angle
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use max_app::qa::png::{self, Image};
use max_app::{App, AppConfig};
use max_core::enums::{unit_flags, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::{PathStep, Point};
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::unit::{GroundPath, UnitPath};
use max_core::unit_order::paths_get_angle;
use max_core::TeamClan;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn save(app: &App, dir: &Path, name: &str) -> Result<(), String> {
    let frame = app.render_frame();
    let image = Image::from_rgba(frame.width(), frame.height(), frame.pixels());
    png::write(dir.join(name), &image)?;
    println!("кадр {name} ({}×{})", frame.width(), frame.height());
    Ok(())
}

fn main() -> ExitCode {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("qa-out/angle"));

    if let Err(error) = run(&out) {
        eprintln!("angle_demo: {error}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn run(out: &Path) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|error| format!("каталог {}: {error}", out.display()))?;

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("assets"))
        .with_screen(640, 480);
    let mut app = App::new(config);
    app.init_assets()
        .map_err(|error| format!("ассеты игры: {error}"))?;

    app.state_mut().teams[0].team_type = TeamKind::Player;
    app.state_mut().team_units[0] = TeamUnits::with_clan(TeamClan::Random);
    let id = app
        .state_mut()
        .spawn_unit(
            UnitType::Scout,
            TeamId::RED,
            Point::new(10, 10),
            unit_flags::MOBILE_LAND_UNIT | unit_flags::SELECTABLE,
        )
        .ok_or("не удалось выставить Scout")?;
    app.state_mut()
        .unit_mut(id)
        .ok_or("юнит не найден")?
        .visible_to_team = [true; 5];

    // L-shaped route: 2 cells east, then 2 north. The starting angle is east.
    let mut unit = app.state().unit(id).ok_or("юнит не найден")?.clone();
    unit.angle = 2;
    unit.orders = UnitOrder::Move;
    unit.state = UnitOrderState::InProgress;
    unit.path = Some(UnitPath::Ground(GroundPath {
        end_x: 12,
        end_y: 8,
        step_index: 0,
        steps: vec![
            PathStep::new(1, 0),
            PathStep::new(1, 0),
            PathStep::new(0, -1),
            PathStep::new(0, -1),
        ],
    }));
    app.state_mut().write_unit(id, unit);

    app.viewport_mut().center_on_grid(11, 9);
    app.update_visual_units(0.0);

    let mut previous = app.visual_position(id).ok_or("юнит не найден")?;
    let mut shot_east = false;
    let mut shot_turn = false;

    for frame in 0..400 {
        if frame < 8 {
            app.tick();
        }
        app.update_visual_units(1.0 / 60.0);

        let unit = app.state().unit(id).ok_or("юнит не найден")?;
        let visual = app.visual_position(id).ok_or("юнит не найден")?;
        let visual_angle = app.visual_angle(id).ok_or("юнит не найден")?;

        // While the sprite travels along an axial segment, the frame angle must match the
        // direction of movement. We do not check the transition through the turn cell within
        // one frame (both dx and dy) — the direction is ambiguous there.
        let dx = visual.0 - previous.0;
        let dy = visual.1 - previous.1;
        let step_x = i32::from(dx > 1e-4) - i32::from(dx < -1e-4);
        let step_y = i32::from(dy > 1e-4) - i32::from(dy < -1e-4);

        if (step_x == 0) != (step_y == 0) {
            let expected = paths_get_angle(step_x, step_y) as u8;
            if visual_angle != expected {
                return Err(format!(
                    "кадр {frame}: спрайт смещается ({dx}, {dy}), а смотрит {visual_angle} (ожидалось {expected})"
                ));
            }
        }

        if !shot_east && unit.angle == 1 && visual.0 < 12.0 && visual.1 == 10.0 {
            println!(
                "01: sim grid=({}, {}) angle={} (NE), visual=({:.2}, {:.2}) angle={} (E)",
                unit.grid.x, unit.grid.y, unit.angle, visual.0, visual.1, visual_angle
            );
            save(&app, out, "01-east-with-sim-ne.png")?;
            shot_east = true;
        }

        if !shot_turn && visual.1 < 10.0 && visual.1 > 8.5 {
            println!(
                "02: sim grid=({}, {}) angle={}, visual=({:.2}, {:.2}) angle={}",
                unit.grid.x, unit.grid.y, unit.angle, visual.0, visual.1, visual_angle
            );
            save(&app, out, "02-turn.png")?;
            shot_turn = true;
        }

        previous = visual;
    }

    let unit = app.state().unit(id).ok_or("юнит не найден")?;
    let visual = app.visual_position(id).ok_or("юнит не найден")?;
    let visual_angle = app.visual_angle(id).ok_or("юнит не найден")?;

    if unit.grid != Point::new(12, 8) || unit.angle != 0 {
        return Err(format!(
            "финал: grid=({}, {}), angle={} (ожидалось (12, 8), 0)",
            unit.grid.x, unit.grid.y, unit.angle
        ));
    }
    if visual != (12.0, 8.0) || visual_angle != 0 {
        return Err(format!(
            "финальный кадр: visual={visual:?}, angle={visual_angle} (ожидалось (12.0, 8.0), 0)"
        ));
    }

    if !shot_east || !shot_turn {
        return Err("не удалось снять кадры поворота".to_string());
    }

    save(&app, out, "03-stopped-north.png")?;
    println!(
        "03: grid=({}, {}) angle={}, visual={visual:?}, visual_angle={visual_angle}",
        unit.grid.x, unit.grid.y, unit.angle
    );

    Ok(())
}
