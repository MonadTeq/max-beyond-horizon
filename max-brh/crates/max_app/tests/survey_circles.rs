//! Survey overlay: the "this cell was already explored" circles.
//!
//! Oracle:
//! * `Survey_RenderMarkers` (`survey.cpp:123-155`) calls `Survey_RenderMarker` for
//!   **every** cell of the view where `Access_GetModifiedSurfaceType != AIR` and
//!   `CargoMap & mask` (the team's `hash_team_id`) — not only for cells with a deposit;
//! * `Survey_RenderMarker` (`survey.cpp:32-82`) takes
//!   `resource_value = min(material_type & CARGO_MASK, 16)` and loads
//!   `RAWMSK0/FUELMK0/GOLDMK0 + resource_value` at `Gfx_ZoomLevel > 16`; frame `0` is the
//!   circle of an explored cell **without** a deposit. At `Gfx_ZoomLevel <= 16` the
//!   small `SM*MK` set is used and `resource_value == 0` returns early.
//!
//! The tests require the game data (`max-original/MAX.RES`): without it they skip
//! themselves, like the other asset-backed `max_app` frame tests.

use std::path::{Path, PathBuf};

use max_app::{App, AppConfig};
use max_core::enums::Surface;
use max_core::geo::{Point, MAP_HEIGHT, MAP_WIDTH};
use max_core::map::{ResourceKind, ResourceMapEntry};
use max_core::universe::GameUniverse;
use max_core::TeamId;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// All-land map, RED is the player, no resources and no surveyed cells.
fn empty_land_state() -> GameUniverse {
    let mut state = GameUniverse::default();

    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            let _ = state.map_mut().surface.set(x, y, Surface::LAND);
        }
    }

    state
        .team_mut(TeamId::RED)
        .expect("RED slot")
        .team_type = max_core::team::TeamKind::Player;
    let team_units = state
        .team_units_mut(TeamId::RED)
        .expect("RED economy");
    *team_units = max_core::TeamUnits::with_clan(max_core::TeamClan::Random);
    team_units.set_gold(2000);
    state
}

/// `App` with real game assets, viewing the empty land around `(40, 40)`.
fn assets_app_with_land() -> Option<App> {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");
    let patches = repo_path("assets/PATCHES.RES");

    let mut config = AppConfig {
        max_res: max_res.clone(),
        color_pal: color_pal.clone(),
        patches_res: patches,
        ..Default::default()
    };

    // `PATCHES.RES` may live in the C++ oracle's shared directory in this
    // checkout: the same fallback `init_assets` uses.
    let (resolved, _) = max_app::assets::resolve_patches_res(&config);

    if !max_res.is_file() || !color_pal.is_file() || !resolved.is_file() {
        return None;
    }

    config.patches_res = resolved;

    let mut app = App::new(config);
    app.init_assets().expect("game assets");
    app.adopt_universe(empty_land_state());
    app.viewport_mut().center_on_grid(40, 40);

    Some(app)
}

/// How many pixels of `cell` changed between frames (the map is shifted by
/// `MAIN_MAP.ulx/uly` in the windowed frame).
fn changed_pixels_in_cell(
    before: &max_render::buffer::RgbaBuffer,
    after: &max_render::buffer::RgbaBuffer,
    app: &App,
    cell: Point,
) -> usize {
    let area = max_render::layout::MAIN_MAP;
    let viewport = app.map_viewport();
    let rect = viewport.tile_rect(cell.x, cell.y);
    let mut changed = 0;

    for y in rect.uly..rect.lry() {
        for x in rect.ulx..rect.lrx() {
            let (sx, sy) = (area.ulx + x, area.uly + y);

            if before.get(sx, sy) != after.get(sx, sy) {
                changed += 1;
            }
        }
    }

    changed
}

/// A surveyed cell **without** a deposit gets the `*MK0` circle: the scout has been
/// there, even though there is nothing to extract. An unsurveyed cell stays clean.
#[test]
fn a_surveyed_cell_without_a_resource_shows_the_circle() {
    let Some(mut app) = assets_app_with_land() else {
        return;
    };

    // RED has explored two empty cells; `unknown` was never visited.
    let surveyed_a = Point::new(40, 41);
    let surveyed_b = Point::new(41, 40);
    let unknown = Point::new(39, 40);

    for cell in [surveyed_a, surveyed_b] {
        let entry = app
            .state_ref()
            .map()
            .resources
            .get(cell.x, cell.y)
            .with_team_visible(0);
        app.universe_mut().map_mut().resources.set(cell.x, cell.y, entry);
    }

    // No SURVEY button, no scout — the overlay is off and the cells are plain tiles.
    app.hud_mut().select(None);
    let (base, _) = app.render_frame_with_stats();
    assert!(!app.survey_overlay_active());

    // The SURVEY panel button enables `Survey_RenderMarkers`.
    app.hud_mut().panels.survey = true;
    let (overlay, stats) = app.render_frame_with_stats();
    assert!(app.survey_overlay_active());

    for cell in [surveyed_a, surveyed_b] {
        assert!(
            changed_pixels_in_cell(&base, &overlay, &app, cell) > 0,
            "разведанная пустая клетка {cell:?} обязана получить кружок"
        );
    }

    assert_eq!(
        changed_pixels_in_cell(&base, &overlay, &app, unknown),
        0,
        "неразведанная клетка остаётся без маркера"
    );
    assert!(
        stats.resource_icon_cells >= 2,
        "кружки обеих разведанных клеток учтены: {}",
        stats.resource_icon_cells
    );
}

/// At `Gfx_ZoomLevel <= 16` C++ uses the small `SM*MK` markers and returns early for
/// `resource_value == 0` (`survey.cpp:62-64`): an explored empty cell gets no marker,
/// a known deposit still does.
#[test]
fn at_small_zoom_only_known_deposits_get_markers() {
    let Some(mut app) = assets_app_with_land() else {
        return;
    };

    let empty = Point::new(40, 41);
    let deposit = Point::new(41, 40);

    for cell in [empty, deposit] {
        let entry = app
            .state_ref()
            .map()
            .resources
            .get(cell.x, cell.y)
            .with_team_visible(0);
        app.universe_mut().map_mut().resources.set(cell.x, cell.y, entry);
    }

    // A surveyed gold deposit next to the surveyed empty cell.
    app.universe_mut().map_mut().resources.set(
        deposit.x,
        deposit.y,
        ResourceMapEntry::new(9, Some(ResourceKind::Gold), 0b1000),
    );

    app.viewport_mut().set_zoom(16);
    app.hud_mut().select(None);
    let (base, _) = app.render_frame_with_stats();

    app.hud_mut().panels.survey = true;
    let (overlay, stats) = app.render_frame_with_stats();

    assert_eq!(
        changed_pixels_in_cell(&base, &overlay, &app, empty),
        0,
        "на малом масштабе разведанная пустая клетка маркера не получает"
    );
    assert!(
        changed_pixels_in_cell(&base, &overlay, &app, deposit) > 0,
        "малый маркер известного месторождения рисуется и на масштабе 16"
    );
    assert!(
        stats.resource_icon_cells >= 1,
        "малый маркер учтён: {}",
        stats.resource_icon_cells
    );
}
