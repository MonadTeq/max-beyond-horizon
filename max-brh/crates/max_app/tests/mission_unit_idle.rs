//! Regression: units loaded from a V70 mission must not render the shot flash.
//!
//! `UnitInfo::FileLoad` (`unitinfo.cpp:2461-2473`) reads the single V70
//! `recoil_delay` byte into `firing_recoil_frames` for every unit that is not
//! disabled. The scenario mission files carry arbitrary non-zero values there
//! (`SAVE1.CAM` has 1…255 on units whose order is `ORDER_AWAIT`/`ORDER_SENTRY`), so
//! a loaded mission unit starts with a stale recoil counter.
//!
//! The C++ renderer does not show it: the frame is drawn from the stored
//! `image_index`/`turret_image_index` (`UnitInfo::Render`, `unitinfo.cpp:3138`),
//! and only `UnitInfo::PrepareFire` (`:4709`) / `ProgressFire` (`:4824`) swap in
//! the firing frame. Both run exclusively from `UnitsManager_ProcessOrderFire`
//! (`units_manager.cpp:2781-2812`), i.e. while `orders == ORDER_FIRE` and
//! `state == ORDER_STATE_FIRE_IN_PROGRESS`. The port derives the frame from the
//! counter, so [`max_app::render::firing_frame_active`] must apply the same gate;
//! otherwise the whole mission roster is painted firing on its first frame.
//!
//! Fixture: `tests/data/save1_v70.cam` — `max-original/SAVE1.CAM`, campaign 1.

use std::path::{Path, PathBuf};

use max_app::local::LocalSetup;
use max_app::render::{
    draw_unit_sprites, firing_body_frame_index, firing_frame_active, firing_turret_frame_index,
};
use max_app::{App, AppConfig, Assets};
use max_assets::sprite::FrameInfo;
use max_core::enums::{unit_flags, UnitOrder, UnitOrderState};
use max_core::unit::{UnitId, UnitInfo, UnitPresentation};
use max_render::buffer::RgbaBuffer;

/// `max-original/SAVE1.CAM` (campaign 1, 312 units).
const SAVE1_CAMPAIGN: &[u8] = include_bytes!("data/save1_v70.cam");

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// A synthetic `D_*` header with a firing block: only the fields the frame
/// selector reads are meaningful.
fn firing_frame_info() -> FrameInfo {
    FrameInfo {
        image_base: 0,
        image_count: 16,
        turret_image_base: 8,
        turret_image_count: 8,
        firing_image_base: 16,
        firing_image_count: 8,
        connector_image_base: 0,
        connector_image_count: 0,
        angle_offsets: [(0, 0); 8],
    }
}

/// Loads campaign 1 through the public mission loader (the same path a mission
/// start takes: `App::start_local_game_from_save`).
fn load_mission() -> App {
    let mut app = App::new(AppConfig::default());
    app.start_local_game_from_save(&LocalSetup::default(), SAVE1_CAMPAIGN)
        .expect("SAVE1.CAM loads");
    app
}

/// Units with a stale V70 recoil byte and a firing sprite, in load order.
fn stale_firing_units(app: &App) -> Vec<(UnitId, UnitInfo)> {
    app.state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| {
            let unit = app.state_ref().unit(id)?;
            (unit.firing_recoil_frames > 0 && unit.flags & unit_flags::HAS_FIRING_SPRITE != 0)
                .then(|| (id, unit.clone()))
        })
        .collect()
}

/// The presentation gate: the V70 `recoil_delay` byte is not a shot.
#[test]
fn loaded_mission_units_with_stale_recoil_do_not_show_the_firing_frame() {
    let app = load_mission();
    let units = stale_firing_units(&app);

    assert!(
        !units.is_empty(),
        "the fixture must contain a loaded unit with a stale V70 recoil byte"
    );

    let info = firing_frame_info();

    for (id, unit) in units {
        assert_ne!(
            unit.state,
            UnitOrderState::FireInProgress,
            "{id:?}: a loaded mission unit is not in the fire sequence"
        );
        assert!(
            !firing_frame_active(&unit),
            "{id:?} ({:?}): a stale recoil counter must not activate the flash",
            unit.unit_type
        );
        assert_eq!(
            firing_body_frame_index(&info, &unit, unit.angle),
            None,
            "{id:?} ({:?}): the body must keep the idle frame",
            unit.unit_type
        );
        assert_eq!(
            firing_turret_frame_index(&info, &unit),
            None,
            "{id:?} ({:?}): the turret must keep the idle frame",
            unit.unit_type
        );
    }
}

/// Game assets, resolved like `Assets::load` does for the real client: the data
/// directory plus the `PATCHES.RES` fallbacks (`assets/`, `max-cpp-port/assets/`).
fn game_assets() -> Option<Assets> {
    let data_dir = repo_path("max-original");

    if !data_dir.join("MAX.RES").is_file() {
        eprintln!("mission_unit_idle: skipped — no game data next to the repository");
        return None;
    }

    let config = AppConfig::default().with_data_dir(&data_dir);

    match Assets::load(&config) {
        Ok(assets) => Some(assets),
        Err(error) => {
            eprintln!("mission_unit_idle: skipped — {error}");
            None
        }
    }
}

fn render(assets: &Assets, unit: &UnitInfo, presentation: &UnitPresentation) -> RgbaBuffer {
    let mut frame = RgbaBuffer::new(256, 256);
    let stats = draw_unit_sprites(assets, unit, presentation, &mut frame, (128, 128), 64)
        .expect("mission unit sprite");
    assert!(stats.unit > 0, "the mission unit body is not drawn");
    frame
}

/// The frame test: a loaded mission unit draws exactly the idle frame, while the
/// same unit inside the fire sequence still draws the flash.
#[test]
fn loaded_mission_unit_renders_the_idle_frame_and_still_flashes_when_firing() {
    let assets = game_assets().expect(concat!("test fixture is missing: ", stringify!(game_assets())));

    let app = load_mission();
    let units = stale_firing_units(&app);

    let (id, unit, presentation) = units
        .into_iter()
        .find_map(|(id, unit)| {
            let sprite = assets.sprite(unit.unit_type)?;
            let info = sprite.frame_info?;
            if info.firing_image_count == 0 {
                return None;
            }
            let presentation = app
                .state_ref()
                .presentation(id)
                .cloned()
                .unwrap_or_default();
            Some((id, unit, presentation))
        })
        .expect("the fixture must contain a stale-recoil unit with firing frames");

    // Loaded mission state: the idle frame, byte for byte.
    let loaded = render(&assets, &unit, &presentation);
    let mut idle = unit.clone();
    idle.firing_recoil_frames = 0;
    let idle_frame = render(&assets, &idle, &presentation);
    assert!(
        loaded.pixels() == idle_frame.pixels(),
        "{id:?} ({:?}): a loaded mission unit must render its idle frame",
        unit.unit_type
    );

    // The flash itself is unaffected: inside `ORDER_FIRE`/`FIRE_IN_PROGRESS` the
    // same counter paints the firing frame.
    let mut firing = unit.clone();
    firing.orders = UnitOrder::Fire;
    firing.state = UnitOrderState::FireInProgress;
    let firing_frame = render(&assets, &firing, &presentation);
    assert!(
        loaded.pixels() != firing_frame.pixels(),
        "{id:?} ({:?}): the firing sequence must still draw the shot flash",
        unit.unit_type
    );
}
