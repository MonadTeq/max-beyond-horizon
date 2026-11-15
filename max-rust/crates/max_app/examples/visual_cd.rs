//! Кадры-доказательства пунктов C/D: миникарта с FOV/2x, маркеры ресурсов,
//! экран выбора планеты и курсоры.
//!
//! ```sh
//! cargo run -p max_app --example visual_cd -- ../build/frame-cd
//! ```
//!
//! Пишет три PPM:
//! * `<prefix>-game.ppm` — окно 1:1: карта с маркерами ресурсов, миникарта
//!   (FOV-маска, рамка камеры, точки юнитов), панели;
//! * `<prefix>-planets.ppm` — форма создания игры с сеткой 24 планет
//!   (`SNOW_PIC + world/6`) и подсветкой выбранной;
//! * `<prefix>-cursors.ppm` — контактный лист всех 30 курсоров из `MAX.RES`.

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

    // FOV: помечаем исследованной левую часть карты — на миникарте виден
    // перепад «исследовано/тёмная маска» (`Access_UpdateMinimapFogOfWar`).
    let mut complete = [0i8; MAP_TILES];
    for y in 0..112 {
        for x in 0..112 {
            if x < 82 {
                complete[y * 112 + x] = 1;
            }
        }
    }
    let team_index = usize::from(app.player_team().0);
    app.state_mut().ai.heat_maps[team_index].load_v70(&complete, &[0; MAP_TILES], &[0; MAP_TILES]);

    // Выбираем юнита и центрируем камеру на клетке с ресурсом: так в кадр
    // попадают и маркеры, и точки юнитов на миникарте.
    let selected = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state().unit(id))
        .find(|unit| unit.team == TeamId::RED && unit.hits > 0)
        .map(|unit| (unit.id, unit.grid));
    let target = (0..112)
        .flat_map(|y| (0..112).map(move |x| (x, y)))
        .find(|&(x, y)| {
            let entry = app.state().map.resource_at(x, y);
            entry.amount() > 0 && entry.is_visible_to(0)
        })
        .unwrap_or((56, 56));

    if let Some((id, grid)) = selected {
        app.hud_mut().select(Some(id));
        app.viewport_mut().center_on_grid(grid.x, grid.y);
        app.hud_mut().panels.cursor_grid = Some((grid.x, grid.y));
    }

    // SURVEY: показать маркеры всех клеток (как кнопка SURVEY в оригинале).
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

    // 2x-миникарта (`MIN2X_ON`): область 56×56 вокруг центра камеры.
    app.hud_mut().panels.minimap_2x = true;
    let (frame_2x, _) = app.render_frame_with_stats();
    let game_2x_path = with_suffix(&prefix, "-game-2x.ppm");
    write_ppm(&game_2x_path, &frame_2x)?;
    println!("game 2x: -> {}", game_2x_path.display());
    app.hud_mut().panels.minimap_2x = false;

    // Экран выбора планеты: 24 картинки `SNOW_PIC + world/6` в сетке 8×3.
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
        },
    );
    let planets_path = with_suffix(&prefix, "-planets.ppm");
    write_ppm(&planets_path, &planets)?;
    println!("planets: {} -> {}", PLANET_COUNT, planets_path.display());

    // Контактный лист курсоров: все 30 ресурсов, первые кадры.
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

/// Лист 8×4: каждый курсор в ячейке 80×110, подпись — имя ресурса.
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
