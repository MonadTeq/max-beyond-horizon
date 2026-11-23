//! The `COLORS` button — the **team** color frame (red/green/blue/gray),
//! 1:1 per the C++ oracle.
//!
//! Oracle:
//! * `DrawMap_RenderUnitDisplays` (`drawmap.cpp:581-598`): with
//!   `GameManager_DisplayButtonColors` and `!(flags & STATIONARY)` around the unit
//!   `DrawMap_RenderColorsDisplay` (`drawmap.cpp:511`) is drawn in
//!   `COLOR_RED`/`COLOR_GREEN`/`COLOR_BLUE`/`0xFF`/`COLOR_DERELICT` — by the bits
//!   `HASH_TEAM_*` in `unit->flags`;
//! * `UnitInfo::UnitInfo` (`unitinfo.cpp:179`):
//!   `flags = unit_flags | team_units->hash_team_id`;
//! * `ResourceManager_InitTeamUnits` (`resource_manager.cpp:969-981`):
//!   RED = `HASH_TEAM_RED` 0x2000, GREEN = 0x1000, BLUE = 0x800, GRAY = 0x400.
//!
//! `team_clan` in C++ does not affect the frame color: it defines the mine clan frames
//! (`unitinfo.cpp:282`) and the clan trade modifiers. The "clan color" in the sense
//! of the COLORS bar is the team slot color from `hash_team_id`. The port does not store
//! these bits in `flags` (they are part of the reference digest) and restores the same
//! color from the `unit.team → color` mapping, while bits from saves are honoured.
//!
//! Tests are skipped if there is no game data (`max-original/`) nearby.

use std::path::{Path, PathBuf};

use max_app::overlays::{
    team_color_index, team_frame_color, COLOR_BLUE, COLOR_DERELICT, COLOR_GRAY, COLOR_GREEN,
    COLOR_RED,
};
use max_app::panels::UiPalette;
use max_app::{App, AppConfig};
use max_core::enums::UnitType;
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_core::TeamClan;
use max_render::buffer::RgbaBuffer;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn app_with_assets() -> Option<App> {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");
    let patches = repo_path("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("overlay_colors: пропуск — нет данных игры рядом с репозиторием");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("assets"))
        .with_screen(640, 640);
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");
    for index in 0..4 {
        app.universe_mut().teams_mut()[index].team_type = TeamKind::Player;
        app.universe_mut().team_units_table_mut()[index] = max_core::TeamUnits::with_clan(TeamClan::Random);
    }
    for index in 4..app.state_ref().teams().len() {
        app.universe_mut().teams_mut()[index].team_type = TeamKind::None;
    }
    app.viewport_mut().set_zoom(64);

    Some(app)
}

/// How many pixels of the frame are exactly this color (the `COLORS` frame — 1 px).
fn color_pixel_count(frame: &RgbaBuffer, color: max_render::buffer::Rgba) -> usize {
    frame
        .pixels()
        .chunks_exact(4)
        .filter(|pixel| pixel[0] == color.r && pixel[1] == color.g && pixel[2] == color.b)
        .count()
}

#[test]
fn colors_frame_uses_the_team_color() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let teams = [TeamId::RED, TeamId::GREEN, TeamId::BLUE, TeamId::GRAY];
    let expected = [COLOR_RED, COLOR_GREEN, COLOR_BLUE, COLOR_GRAY];

    let mut ids = Vec::new();
    for (index, team) in teams.iter().enumerate() {
        let id = app
            .universe_mut()
            .spawn_unit(
                UnitType::Tank,
                *team,
                Point::new(22 + index as i16 * 3, 20),
                0,
            )
            .expect("tank");
        // Visually visible to the player (RED) — otherwise the map does not draw it.
        if let Some(unit) = app.universe_mut().unit_mut(id) {
            unit.visible_to_team = [true; 5];
        }
        assert_eq!(
            team_color_index(*team),
            expected[index],
            "{team:?}: индекс палитры цвета команды"
        );
        ids.push(id);
    }
    app.viewport_mut().center_on_grid(26, 20);

    let palette = UiPalette::from_assets(app.assets().expect("assets")).expect("палитра окна");

    // Without the COLORS button there are no frames.
    let without = app.render_frame();
    app.hud_mut().panels.colors = true;
    let (with, stats) = app.render_frame_with_stats();
    assert_eq!(stats.units_drawn, 4, "все четыре юнита в камере");
    let mut diff_colors: std::collections::HashMap<(u8, u8, u8), usize> =
        std::collections::HashMap::new();
    for (a, b) in without
        .pixels()
        .chunks_exact(4)
        .zip(with.pixels().chunks_exact(4))
    {
        if a != b {
            *diff_colors.entry((b[0], b[1], b[2])).or_default() += 1;
        }
    }
    assert!(!diff_colors.is_empty(), "COLORS обязан что-то нарисовать");
    for index in 0..teams.len() {
        let color = palette.rgba(expected[index]);
        assert!(
            diff_colors.contains_key(&(color.r, color.g, color.b)),
            "{:?}: в кадре нет пикселей цвета {} — рамка не нарисована ({diff_colors:?})",
            teams[index],
            expected[index]
        );
    }

    for (index, id) in ids.iter().enumerate() {
        let unit = app.state_ref().unit(*id).expect("unit");
        let color = team_frame_color(&palette, unit);
        assert_eq!(
            color,
            palette.rgba(expected[index]),
            "{:?}: цвет рамки — {}",
            unit.team,
            expected[index]
        );

        // The frame is 4 perimeter lines: there are pixels of the team color in the frame with
        // COLORS enabled more (and there are at least enough for the frame).
        let before = color_pixel_count(&without, color);
        let after = color_pixel_count(&with, color);
        assert!(
            after > before,
            "{:?}: рамка цвета {} не нарисована (было {before}, стало {after})",
            unit.team,
            expected[index]
        );
    }

    // The loaded save carries the `HASH_TEAM_*` bits directly in `flags` — they are honoured
    // the same way as in C++ (`DrawMap_RenderUnitDisplays`).
    let mut flagged = app.state_ref().unit(ids[0]).expect("unit").clone();
    flagged.flags |= max_core::enums::unit_flags::HASH_TEAM_BLUE;
    assert_eq!(
        team_frame_color(&palette, &flagged),
        palette.rgba(COLOR_BLUE),
        "флаги из сейва перебивают цвет команды"
    );
}

#[test]
fn alien_units_fall_back_to_derelict_and_stationary_is_skipped() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let alien = app
        .universe_mut()
        .spawn_unit(UnitType::AlienTank, TeamId::ALIEN, Point::new(20, 30), 0)
        .expect("alien tank");
    if let Some(unit) = app.universe_mut().unit_mut(alien) {
        unit.visible_to_team = [true; 5];
    }
    app.viewport_mut().center_on_grid(20, 30);
    app.hud_mut().panels.colors = true;

    let palette = UiPalette::from_assets(app.assets().expect("assets")).expect("палитра");
    let unit = app.state_ref().unit(alien).expect("alien");
    assert_eq!(team_color_index(TeamId::ALIEN), COLOR_DERELICT);
    assert_eq!(
        team_frame_color(&palette, unit),
        palette.rgba(COLOR_DERELICT),
        "пришельцы/derelict — 0xA9"
    );

    // C++ draws COLORS only for **non**-stationary (`!(flags & STATIONARY)`).
    app.universe_mut()
        .spawn_unit(UnitType::LightVehiclePlant, TeamId::RED, Point::new(60, 60), 0)
        .expect("factory");
    app.hud_mut().panels.colors = false;
    let without = app.render_frame();
    app.hud_mut().panels.colors = true;
    let with = app.render_frame();
    assert_eq!(
        color_pixel_count(&with, palette.rgba(COLOR_RED)),
        color_pixel_count(&without, palette.rgba(COLOR_RED)),
        "стационарному заводу рамка COLORS не рисуется"
    );
}
