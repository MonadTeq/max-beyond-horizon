//! Frame-level z-order regression: an aircraft is **always** drawn over the
//! ground/sea/building sprite of its cell.
//!
//! # Oracle
//!
//! `UnitInfoGroup::RenderGroups` (`max-cpp-port/src/unitinfogroup.cpp:181-237`)
//! does not sort every sprite by its cell: it draws whole groups in a fixed
//! order — ground covers and mobile land/sea units, then stationary buildings,
//! then `air_units_on_ground`, then rockets, then `air_units_in_air`, then the
//! explosion particles. The aircraft groups are therefore always above every
//! ground/sea/building sprite, no matter where the aircraft is.
//!
//! # Defect (fixed)
//!
//! The port sorted units by `(y, x)` of the **rounded visual cell** only
//! (`draw_order`). For a resting aircraft that cell equals the cell of the unit
//! under it (the list order of the air unit broke the tie), but:
//!
//! * a 2×2 building is drawn from its center pixel (`grid * 64 + 63`,
//!   `UnitInfo::UpdateGridPosition`), which rounds to the cell **+1** — an
//!   aircraft over the building's upper/left half got a smaller sort key;
//! * an aircraft crossing a cell border rounds to the next/previous cell while
//!   the unit under it keeps its own cell.
//!
//! In both cases the ground/sea/building sprite was drawn after the aircraft and
//! covered it. The fix adds the oracle's group layer to the sort key
//! (`max_app::render::unit_draw_layer` + `max_render::draw_order_layered`), so
//! the air layers are always drawn last.
//!
//! The tests render the same camera four times — map only, the lower unit alone,
//! the aircraft alone and both — build the **body** masks of the two sprites and
//! compare the overlapping body pixels: the composite has to show the aircraft
//! pixel. (The shadow is not part of the mask: it is a darkening pass, not a
//! sprite, and would only confuse the z-order comparison.)

use std::path::{Path, PathBuf};

use max_app::render::{image_base_for, unit_frame_index_with_animation, FrameAnimation};
use max_app::{App, AppConfig};
use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitId;
use max_core::universe::GameUniverse;
use max_render::buffer::{Rgba, RgbaBuffer};
use max_render::palette::{ColorLut, TeamColors, FULL_BRIGHTNESS_LEVEL};
use max_render::sprite_atlas::SpriteRenderer;

/// Path inside the repository (`max-brh/crates/max_app/../../..`).
fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// The game data is required by the frame tests (sprite z-order is only visible
/// with real sprites): skip when it is not next to the repository.
fn game_data_present() -> bool {
    repo_path("max-original/MAX.RES").is_file()
        && repo_path("max-original/COLOR.PAL").is_file()
        && repo_path("tests/fixtures/assets/PATCHES.RES").is_file()
}

/// Cell both units stand on (or hover over).
const CELL: Point = Point::new(20, 20);

/// A 448×448 full-screen frame (no window chrome) with the assets of the game
/// and an empty match camera centered on [`CELL`].
fn clean_app() -> Box<App> {
    let mut universe = GameUniverse::with_setup(
        GameUniverse::default().snapshot().map().clone(),
        [
            TeamKind::Player,
            TeamKind::None,
            TeamKind::None,
            TeamKind::None,
        ],
    );
    universe.set_player_team(TeamId::RED);

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("tests/fixtures/assets"))
        .with_screen(448, 448);
    let mut app = Box::new(App::new(config));
    app.init_assets().expect("assets of the game");
    app.adopt_universe(universe);
    app.viewport_mut().set_zoom(64);
    app.viewport_mut().center_on_grid(CELL.x, CELL.y);

    app
}

/// Spawns a unit with the `SC_UNITS` flags of its type.
fn spawn(app: &mut App, unit_type: UnitType) -> UnitId {
    app.universe_mut()
        .spawn_unit(unit_type, TeamId::RED, CELL, 0)
        .expect("unit spawned")
}

/// Moves a flying aircraft inside [`CELL`] so that its **rounded visual cell**
/// is `CELL - 1` in both axes while the cell it hovers over is still `CELL`.
///
/// The pixel is one left/up of the cell center (`UnitInfo::x/y` of a plane
/// crossing the cell border, `AirPath::Execute`), not a teleport: the ordinates
/// are written directly the way the simulation writes them. The center itself
/// rounds half-up (`20.5 -> 21`, `x / 64 = 20.5`), so `+31` is the last pixel of
/// the previous cell.
fn fly_aircraft_off_cell_center(app: &mut App, id: UnitId) {
    let mut unit = app.state_ref().unit(id).cloned().expect("aircraft");

    unit.x = (CELL.x as u16) * 64 + 31;
    unit.y = (CELL.y as u16) * 64 + 31;
    unit.orders = UnitOrder::Move;
    unit.state = UnitOrderState::InProgress;
    unit.angle = 2;
    app.universe_mut().write_unit(id, unit);
}

/// The **body** sprite of a unit drawn alone on a transparent buffer at its
/// `UnitInfo::x/y` anchor — the same palette, team LUT, brightness and frame the
/// map renderer uses (`draw_unit_sprites_with_animation` without the shadow).
fn body_mask(app: &App, id: UnitId) -> RgbaBuffer {
    let assets = app.assets().expect("assets");
    let unit = app.state_ref().unit(id).cloned().expect("unit");
    let presentation = app
        .state_ref()
        .presentation(id)
        .cloned()
        .unwrap_or_default();
    let team_clan = app
        .state_ref()
        .teams()
        .get(unit.team.0 as usize)
        .map(|team| team.team_clan);
    let image_base = image_base_for(&unit, team_clan);
    let body_angle = app.visual_angle(id).unwrap_or(unit.angle);
    let sprite = assets.sprite(unit.unit_type).expect("sprite");
    let sentry_frames = sprite
        .frame_info
        .map(|info| usize::from(info.image_count))
        .filter(|frames| *frames > 0)
        .unwrap_or(sprite.sheet.len());
    let index = unit_frame_index_with_animation(
        &unit,
        image_base,
        sprite.sheet.len(),
        sentry_frames,
        body_angle,
        FrameAnimation::default(),
        true,
    );
    let frame = sprite.frame_at(index).expect("body frame");
    let colors = TeamColors::ALL
        .get(unit.team.0 as usize)
        .copied()
        .unwrap_or(TeamColors::Derelict);
    let level = presentation.brightness.min(FULL_BRIGHTNESS_LEVEL);
    let renderer = SpriteRenderer::new(
        &assets.system_palette,
        &assets.brightness,
        ColorLut::for_team(colors),
        level,
    );
    let (anchor_x, anchor_y) = app
        .viewport()
        .world_to_screen(i32::from(unit.x), i32::from(unit.y));
    let mut buffer = RgbaBuffer::filled(448, 448, Rgba::TRANSPARENT);

    renderer.draw_frame_scaled(&mut buffer, frame, (anchor_x, anchor_y), 64);

    buffer
}

/// Overlapping **body** pixels of the lower unit and the aircraft must show the
/// aircraft in the composite frame.
///
/// `map_only` — the empty camera, `ground` — the lower unit alone, `air` — the
/// aircraft alone, `both` — both units; `lower_body`/`air_body` — the body masks
/// of the two sprites (see [`body_mask`]). A pixel counted here is an opaque body
/// pixel of both sprites with different colors, so the z-order is visible on it.
fn assert_aircraft_on_top(
    map_only: &RgbaBuffer,
    ground: &RgbaBuffer,
    air: &RgbaBuffer,
    both: &RgbaBuffer,
    lower_body: &RgbaBuffer,
    air_body: &RgbaBuffer,
    what: &str,
) {
    let (width, height) = (map_only.width() as i32, map_only.height() as i32);
    let mut compared = 0;

    for y in 0..height {
        for x in 0..width {
            let (Some(lower_pixel), Some(air_pixel)) = (lower_body.get(x, y), air_body.get(x, y))
            else {
                continue;
            };

            // Both sprites have to be opaque on the pixel, with different colors:
            // then and only then the z-order is visible on it.
            if lower_pixel.a == 0 || air_pixel.a == 0 || lower_pixel == air_pixel {
                continue;
            }

            // The lower sprite must actually be drawn there in its own frame, and
            // the aircraft too — otherwise the camera clipped one of them.
            if ground.get(x, y) == map_only.get(x, y) || air.get(x, y) == map_only.get(x, y) {
                continue;
            }

            compared += 1;

            assert_eq!(
                both.get(x, y),
                air.get(x, y),
                "{what}: ({x}, {y}): the aircraft must be drawn over the ground/sea/building \
                 sprite of its cell (composite {:?}, aircraft {:?}, lower sprite {:?})",
                both.get(x, y),
                air.get(x, y),
                ground.get(x, y)
            );
        }
    }

    assert!(
        compared > 32,
        "{what}: the test needs overlapping aircraft/lower body pixels, found {compared} — \
         did the sprites stop being drawn?"
    );
}

/// An aircraft over a ground unit: the same cell (list order already put air
/// last) and the aircraft's pixel rounded into the cell left/up of the unit
/// (the regression: the plain cell sort drew the tank last).
#[test]
fn an_aircraft_is_drawn_over_a_ground_unit() {
    if !game_data_present() {
        eprintln!("air_z_order: нет данных — нет данных игры рядом с репозиторием");
        return;
    }

    let mut app = clean_app();
    let map_only = app.render_frame();

    let tank = spawn(&mut app, UnitType::Tank);
    let tank_frame = app.render_frame();

    let fighter = spawn(&mut app, UnitType::Fighter);
    fly_aircraft_off_cell_center(&mut app, fighter);
    let both = app.render_frame();
    let tank_body = body_mask(&app, tank);
    let fighter_body = body_mask(&app, fighter);

    app.universe_mut().destroy_unit(tank);
    let fighter_frame = app.render_frame();

    assert_aircraft_on_top(
        &map_only,
        &tank_frame,
        &fighter_frame,
        &both,
        &tank_body,
        &fighter_body,
        "tank",
    );
}

/// A resting aircraft over a tank: the visual cells coincide, so this case is
/// the conservative guard of the same rule.
#[test]
fn a_resting_aircraft_is_drawn_over_the_unit_of_its_cell() {
    if !game_data_present() {
        eprintln!("air_z_order: нет данных — нет данных игры рядом с репозиторием");
        return;
    }

    let mut app = clean_app();
    let map_only = app.render_frame();

    let tank = spawn(&mut app, UnitType::Tank);
    let tank_frame = app.render_frame();

    let fighter = spawn(&mut app, UnitType::Fighter);
    let both = app.render_frame();
    let tank_body = body_mask(&app, tank);
    let fighter_body = body_mask(&app, fighter);

    app.universe_mut().destroy_unit(tank);
    let fighter_frame = app.render_frame();

    assert_aircraft_on_top(
        &map_only,
        &tank_frame,
        &fighter_frame,
        &both,
        &tank_body,
        &fighter_body,
        "tank",
    );
}

/// A 2×2 building: its sprite is anchored on the center pixel
/// (`grid * 64 + 63`), so a plain cell sort puts the building after an aircraft
/// hovering over its upper half.
#[test]
fn an_aircraft_is_drawn_over_a_building() {
    if !game_data_present() {
        eprintln!("air_z_order: нет данных — нет данных игры рядом с репозиторием");
        return;
    }

    let mut app = clean_app();
    let map_only = app.render_frame();

    let building = spawn(&mut app, UnitType::MiningStation);
    let building_frame = app.render_frame();

    let fighter = spawn(&mut app, UnitType::Fighter);
    fly_aircraft_off_cell_center(&mut app, fighter);
    let both = app.render_frame();
    let building_body = body_mask(&app, building);
    let fighter_body = body_mask(&app, fighter);

    app.universe_mut().destroy_unit(building);
    let fighter_frame = app.render_frame();

    assert_aircraft_on_top(
        &map_only,
        &building_frame,
        &fighter_frame,
        &both,
        &building_body,
        &fighter_body,
        "building",
    );
}
