//! Ship rock on water — the port of `UnitInfo::ShakeWater`
//! (`max-cpp-port/src/unitinfo.cpp:4592-4634`).
//!
//! # Oracle
//!
//! `UnitInfo::Animate` (`unitinfo.cpp:5056-5067`) calls `ShakeWater` once per
//! order tick for `MOBILE_SEA_UNIT`/`SEAMINE` units whose
//! `Access_GetModifiedSurfaceType(grid) == SURFACE_TYPE_WATER`. Every 32nd tick
//! `ShakeWater` applies `OffsetDrawZones(±DIRECTION_OFFSETS[moved / 64])`:
//! `+DIR` while `moved % 64 < 32`, `-DIR` on the next 32nd tick — the ship
//! rocks one pixel around its anchor and the direction rotates every 64 ticks.
//! The shadow is shifted twice as far (`shadow_offset -= offset`,
//! `unitinfo.cpp:4622-4623`). `dt = 0` (QA/headless) has no such call, so the
//! golden frames do not drift.
//!
//! # Defect (fixed)
//!
//! The port had only the aircraft flight bob (`ShakeAir`); sea units stood
//! completely still on water. The frame now keeps the C++ `moved` counter at the
//! order-tick pace ([`max_app::ORDER_TICK_SECONDS`]) — frame only, not part of
//! the snapshot/CRC.

use std::path::{Path, PathBuf};

use max_app::render::{image_base_for, unit_frame_index_with_animation, FrameAnimation};
use max_app::{App, AppConfig, ORDER_TICK_SECONDS};
use max_core::enums::{Surface, UnitType};
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

/// The game data is needed by the frame test (the rock is only visible on a real
/// sprite): skip it when the data is not next to the repository.
fn game_data_present() -> bool {
    repo_path("max-original/MAX.RES").is_file()
        && repo_path("max-original/COLOR.PAL").is_file()
        && repo_path("tests/fixtures/assets/PATCHES.RES").is_file()
}

/// Cell of the ship in all tests.
const CELL: Point = Point::new(20, 20);
/// A dry cell for the "no water — no rock" case.
const LAND_CELL: Point = Point::new(24, 20);

/// The application without assets: the bob clock does not read sprites.
fn plain_app() -> Box<App> {
    Box::new(App::new(AppConfig::default()))
}

/// The application with the game assets and a 448×448 full-screen frame.
fn app_with_assets() -> Box<App> {
    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("tests/fixtures/assets"))
        .with_screen(448, 448);
    let mut app = Box::new(App::new(config));
    app.init_assets().expect("assets of the game");

    app
}

/// A clean universe with [`CELL`] under water and [`LAND_CELL`] dry.
fn water_universe() -> GameUniverse {
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
    assert!(
        universe
            .map_mut()
            .surface
            .set(CELL.x, CELL.y, Surface::WATER),
        "the cell must be on the map"
    );
    assert!(
        universe
            .map_mut()
            .surface
            .set(LAND_CELL.x, LAND_CELL.y, Surface::LAND),
        "the cell must be on the map"
    );
    universe
}

/// Spawns a unit with the `SC_UNITS` flags of its type on `grid`.
fn spawn(app: &mut App, unit_type: UnitType, grid: Point) -> UnitId {
    app.universe_mut()
        .spawn_unit(unit_type, TeamId::RED, grid, 0)
        .expect("unit spawned")
}

/// Advances the visual clock `ticks` order ticks.
fn advance(app: &mut App, ticks: u32) {
    for _ in 0..ticks {
        app.update_visual_units(ORDER_TICK_SECONDS);
    }
}

/// The C++ `ShakeWater` sequence: one pixel out along N, back after 32 ticks, the
/// next direction (NE) after 64, and so on. A land unit keeps its anchor.
#[test]
fn a_sea_unit_on_water_rocks_and_a_land_unit_does_not() {
    let mut app = plain_app();
    app.adopt_universe(water_universe());

    let ship = spawn(&mut app, UnitType::Gunboat, CELL);
    let grounded_ship = spawn(&mut app, UnitType::Gunboat, LAND_CELL);
    let tank = spawn(&mut app, UnitType::Tank, Point::new(CELL.x + 2, CELL.y));

    app.update_visual_units(0.0);
    assert_eq!(
        app.sea_bob_offset(ship),
        (0, 0),
        "dt = 0 (QA/headless): статичный кадр"
    );

    // `moved = 1`: the first half of the block pushes one pixel north.
    advance(&mut app, 1);
    assert_eq!(app.sea_bob_offset(ship), (0, -1));

    // `moved = 32`: the return offset, the hull is back on its anchor.
    advance(&mut app, 31);
    assert_eq!(app.sea_bob_offset(ship), (0, 0), "качание возвращается");

    // `moved = 64`: the next direction (NE) starts.
    advance(&mut app, 32);
    assert_eq!(app.sea_bob_offset(ship), (1, -1));

    // `moved = 96` — back again; `moved = 128` — east.
    advance(&mut app, 32);
    assert_eq!(app.sea_bob_offset(ship), (0, 0));
    advance(&mut app, 32);
    assert_eq!(app.sea_bob_offset(ship), (1, 0));

    assert_eq!(
        app.sea_bob_offset(grounded_ship),
        (0, 0),
        "корабль на суше не качается"
    );
    assert_eq!(app.sea_bob_offset(tank), (0, 0), "танк не качается");
}

/// The rock is frame only: it does not change the snapshot/CRC.
#[test]
fn the_water_bob_does_not_change_the_snapshot() {
    let mut app = plain_app();
    app.adopt_universe(water_universe());
    let ship = spawn(&mut app, UnitType::Gunboat, CELL);

    let before = app.snapshot();

    advance(&mut app, 200);

    assert_eq!(
        app.snapshot(),
        before,
        "качание кораблей — только презентация"
    );
    assert_ne!(app.sea_bob_offset(ship), (0, 0));
}

/// `settings["effects"] = false` freezes the animation (`UnitInfo::Animate`
/// checks the setting, `unitinfo.cpp:5057`).
#[test]
fn effects_off_stops_the_water_bob() {
    let mut app = plain_app();
    app.adopt_universe(water_universe());
    let ship = spawn(&mut app, UnitType::Gunboat, CELL);

    let mut preferences = app.preferences().clone();
    preferences.effects = false;
    app.set_preferences(preferences);

    advance(&mut app, 8);

    assert_eq!(
        app.sea_bob_offset(ship),
        (0, 0),
        "effects = false: спрайт не качается"
    );
}

/// **Body** sprite of the sea transport drawn alone at `anchor` — the same
/// palette, team LUT, brightness and frame the map renderer uses, without the
/// shadow and the turret (both are darkening/overlay passes; the z-order is read
/// from the body pixels).
fn body_mask(app: &App, id: UnitId, anchor: (i32, i32)) -> RgbaBuffer {
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
    let renderer = SpriteRenderer::new(
        &assets.system_palette,
        &assets.brightness,
        ColorLut::for_team(colors),
        presentation.brightness.min(FULL_BRIGHTNESS_LEVEL),
    );
    let mut buffer = RgbaBuffer::filled(448, 448, Rgba::TRANSPARENT);

    renderer.draw_frame_scaled(&mut buffer, frame, anchor, 64);

    buffer
}

/// Frame-level check: the ship sprite is drawn one pixel above its cell anchor
/// (the `ShakeWater` offset) and the rock changes the frame.
///
/// The hull is a `SEATRANS` — a sea type without a turret — so every opaque body
/// pixel of the frame equals the body sprite drawn at the rock offset (the
/// shadow is drawn **under** the body, `SpriteRenderer::draw_unit_scaled`).
#[test]
fn the_ship_sprite_is_drawn_at_the_rock_offset() {
    if !game_data_present() {
        eprintln!("sea_bob: нет данных — нет данных игры рядом с репозиторием");
        return;
    }

    let mut app = app_with_assets();
    app.adopt_universe(water_universe());
    let ship = spawn(&mut app, UnitType::SeaTransport, CELL);
    app.viewport_mut().set_zoom(64);
    app.viewport_mut().center_on_grid(CELL.x, CELL.y);

    // Rest: the clock is cleared by `dt = 0`; the single 42 ms frame below does
    // not step the palette cycles either, so the frames differ only by the rock.
    app.update_visual_units(0.0);
    let rest = app.render_frame();
    assert_eq!(app.sea_bob_offset(ship), (0, 0));
    assert!(
        app.animated_palette().is_none(),
        "the palette must not step: the frame diff has to be the rock alone"
    );

    app.update_visual_units(ORDER_TICK_SECONDS);
    let bob = app.sea_bob_offset(ship);
    assert_eq!(bob, (0, -1), "первый шаг качания — на пиксель севернее");
    assert!(
        app.animated_palette().is_none(),
        "the palette must not step on the 42 ms tick"
    );
    let bobbed = app.render_frame();

    let unit = app.state_ref().unit(ship).cloned().expect("ship");
    let (anchor_x, anchor_y) = app
        .viewport()
        .world_to_screen(i32::from(unit.x), i32::from(unit.y));
    let body = body_mask(&app, ship, (anchor_x + bob.0, anchor_y + bob.1));
    let still = body_mask(&app, ship, (anchor_x, anchor_y));

    let (width, height) = (448i32, 448i32);
    let mut compared = 0;
    let mut moved = 0;

    for y in 0..height {
        for x in 0..width {
            let Some(pixel) = body.get(x, y) else {
                continue;
            };

            if pixel.a == 0 {
                continue;
            }

            compared += 1;
            assert_eq!(
                bobbed.get(x, y),
                Some(pixel),
                "({x}, {y}): the ship sprite must stand on the `ShakeWater` offset \
                 ({bob:?}) from its `UnitInfo::x/y` anchor"
            );

            if rest.get(x, y) != Some(pixel) {
                moved += 1;
            }
        }
    }

    assert!(compared > 100, "the ship sprite must cover pixels");
    assert!(
        moved > 0,
        "the rock must move the ship sprite: the rest frame has to differ on the \
         offset pixels"
    );

    // The still body must not be where the frame draws the rocking one: this is
    // the negative control of the offset direction.
    let mut displaced = 0;
    for y in 0..height {
        for x in 0..width {
            if let (Some(pixel), Some(other)) = (still.get(x, y), body.get(x, y)) {
                if pixel.a != 0 && other.a == 0 {
                    displaced += 1;
                }
            }
        }
    }
    assert!(displaced > 0, "the rock must displace the sprite");
}
