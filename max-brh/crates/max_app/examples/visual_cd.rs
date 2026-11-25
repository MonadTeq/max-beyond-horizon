//! Proof frames for items C/D: the minimap with FOV/2x, resource markers,
//! the planet selection screen and the cursors.
//!
//! ```sh
//! cargo run -p max_app --example visual_cd -- ../build/frame-cd
//! ```
//!
//! Writes three PPM files:
//! * `<prefix>-game.ppm` — 1:1 window: map with resource markers, minimap
//!   (FOV mask, camera frame, unit dots), panels;
//! * `<prefix>-planets.ppm` — game creation form with a 24-planet grid
//!   (`SNOW_PIC + world/6`) and the selected one highlighted;
//! * `<prefix>-cursors.ppm` — contact sheet of all 30 cursors from `MAX.RES`.

use std::path::{Path, PathBuf};

use max_app::render::write_ppm;
use max_app::{App, AppConfig, NewGameConfig, Screen, ScreenView};
use max_app::{CreateForm, PLANET_COUNT};
use max_core::setup::ResourceLevel;
use max_core::{geo::MAP_TILES, TeamId};
use max_render::buffer::{Rgba, RgbaBuffer};
use max_render::text::draw_text_line;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn main() -> anyhow::Result<()> {
    let prefix = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("frame-cd"));

    let config = AppConfig {
        max_res: repo_path("max-original/MAX.RES"),
        color_pal: repo_path("max-original/COLOR.PAL"),
        patches_res: repo_path("assets/PATCHES.RES"),
        screen_width: 640,
        screen_height: 480,
        ..Default::default()
    };
    let mut app = App::new(config);
    let report = app.init_assets()?;
    let assets = app.assets().expect("ассеты");
    println!(
        "assets: MAX.RES={} маркеры={} курсоры={}/{} картинки планет={}",
        report.max_res_entries,
        assets.resource_markers.loaded(),
        assets.cursors.as_ref().map_or(0, |cursors| cursors.loaded()),
        max_assets::CURSOR_COUNT,
        assets.planet_pictures.len()
    );

    app.new_game(&NewGameConfig {
        raw: ResourceLevel::HIGH,
        fuel: ResourceLevel::HIGH,
        gold: ResourceLevel::HIGH,
        ..NewGameConfig::p2_reference()
    })?;

    // FOV: mark the left part of the map as explored — the minimap then shows
    // the "explored/dark mask" contrast (`Access_UpdateMinimapFogOfWar`).
    let mut complete = [0i8; MAP_TILES];
    for y in 0..112 {
        for x in 0..112 {
            if x < 82 {
                complete[y * 112 + x] = 1;
            }
        }
    }
    let team_index = usize::from(app.player_team().0);
    app.universe_mut().ai_mut().heat_maps[team_index].load_v70(&complete, &[0; MAP_TILES], &[0; MAP_TILES]);

    // Select a unit and center the camera on the resource cell: this way both the
    // markers and the unit dots on the minimap get into the frame.
    let selected = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state_ref().unit(id))
        .find(|unit| unit.team == TeamId::RED && unit.hits > 0)
        .map(|unit| (unit.id, unit.grid));
    let target = (0..112)
        .flat_map(|y| (0..112).map(move |x| (x, y)))
        .find(|&(x, y)| {
            let entry = app.state_ref().map().resource_at(x, y);
            entry.amount() > 0 && entry.is_visible_to(0)
        })
        .unwrap_or((56, 56));

    if let Some((id, grid)) = selected {
        app.hud_mut().select(Some(id));
        app.viewport_mut().center_on_grid(grid.x, grid.y);
        app.hud_mut().panels.cursor_grid = Some((grid.x, grid.y));
    }

    // SURVEY: show the markers of all cells (like the SURVEY button in the original).
    app.hud_mut().panels.survey = true;
    let _ = target;
    let (frame, stats) = app.render_frame_with_stats();
    let game_path = with_suffix(&prefix, "-game.ppm");
    write_ppm(&game_path, &frame)?;
    println!(
        "game: {}x{} tiles={} markers={} units={} -> {}",
        frame.width(),
        frame.height(),
        stats.map_tiles,
        stats.resource_icons,
        stats.units(),
        game_path.display()
    );

    // 2x minimap (`MIN2X_ON`): a 56×56 area around the camera center.
    app.hud_mut().panels.minimap_2x = true;
    let (frame_2x, _) = app.render_frame_with_stats();
    let game_2x_path = with_suffix(&prefix, "-game-2x.ppm");
    write_ppm(&game_2x_path, &frame_2x)?;
    println!("game 2x: -> {}", game_2x_path.display());
    app.hud_mut().panels.minimap_2x = false;

    // Planet selection screen: 24 `SNOW_PIC + world/6` pictures in an 8×3 grid.
    let form = CreateForm {
        world: 12,
        ..Default::default()
    };
    let mut planets = RgbaBuffer::new(640, 480);
    let cell = max_app::ui::planet_cell_rect(640, 480, 12);
    Screen::CreateForm(form).render(
        &mut planets,
        ScreenView {
            width: 640,
            height: 480,
            status: "Click a planet - the World field stays a numeric fallback",
            mouse: Some(max_render::ui::Point::new(cell.x + 8, cell.y + 8)),
            focus: 0,
            app: Some(&app),
            missions: None,
        },
    );
    let planets_path = with_suffix(&prefix, "-planets.ppm");
    write_ppm(&planets_path, &planets)?;
    println!("planets: {} -> {}", PLANET_COUNT, planets_path.display());

    // Cursor contact sheet: all 30 resources, first frames.
    let cursors = render_cursor_sheet(&app);
    let cursors_path = with_suffix(&prefix, "-cursors.ppm");
    write_ppm(&cursors_path, &cursors)?;
    println!("cursors: 30 -> {}", cursors_path.display());

    Ok(())
}

fn with_suffix(prefix: &Path, suffix: &str) -> PathBuf {
    let mut name = prefix.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// 8×4 sheet: each cursor in an 80×110 cell, the caption is the resource name.
fn render_cursor_sheet(app: &App) -> RgbaBuffer {
    use max_app::CursorId;

    let assets = app.assets().expect("ассеты");
    let cursors = assets.cursors.as_ref().expect("курсоры");
    let palette = max_app::render::window_palette(assets);
    let mut buffer = RgbaBuffer::filled(640, 480, Rgba::rgb(24, 24, 32));

    for (index, id) in CursorId::ALL.into_iter().enumerate() {
        let column = (index % 8) as i32;
        let row = (index / 8) as i32;
        let x = column * 80 + 4;
        let y = row * 110 + 4;

        draw_text_line(
            &mut buffer,
            x,
            y,
            id.resource_name(),
            Rgba::rgb(180, 190, 220),
        );

        if let Some(image) = max_app::cursor::cursor_image(cursors, &palette, id, 0) {
            let scale = if image.width > 40 || image.height > 40 {
                2
            } else {
                1
            };

            for dy in 0..image.height as i32 {
                for dx in 0..image.width as i32 {
                    let Some([r, g, b, a]) = image.pixel(dx as u32, dy as u32) else {
                        continue;
                    };

                    if a == 0 {
                        continue;
                    }

                    for sy in 0..scale {
                        for sx in 0..scale {
                            buffer.set(
                                x + dx * scale + sx,
                                y + 12 + dy * scale + sy,
                                Rgba::rgb(r, g, b),
                            );
                        }
                    }
                }
            }
        }
    }

    buffer
}
