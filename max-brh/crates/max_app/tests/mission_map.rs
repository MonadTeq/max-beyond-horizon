//! Campaign mission 1 map: the V70 surface/cargo data and the planet terrain
//! must match the Interplay oracle (`max-original/SAVE1.CAM` + `GREEN_5.WRL`).
//!
//! Defect (fixed here): a mission loaded from V70 (`GAME_STATE_10_LOAD_GAME`,
//! `game_manager.cpp:1929-1961`) kept the planet of the menu / `--world` assets.
//! The oracle's `SaveLoad_LoadFormatV70` calls
//! `ResourceManager_InitInGameAssets(world_index)` (`saveload.cpp:796`,
//! `resource_manager.cpp:1147,1183-1185`): the surface/cargo bytes come from the
//! save (`saveload.cpp:825-829`), while the terrain tiles (`gfx.cpp:175`), the
//! minimap and the music come from `SNOW_1 + world_index`. `SAVE1.CAM` is
//! `world = 16` (Islandia = `GREEN_5`) — not the default `SNOW_1` of
//! [`AppConfig`], so the mission was drawn with the wrong planet's tiles.
//!
//! Gates:
//! 1. [`v70_load_installs_the_map_and_planet_of_the_header`] — a headless load
//!    (no assets) installs the save's surface/cargo bytes and remembers the
//!    header planet. The byte offsets are the V70 layout pinned by
//!    `max_core/tests/phase4_save.rs` (`save.md`).
//! 2. [`mission_start_installs_the_planet_of_the_v70_header`] — the mission
//!    start flow (`MissionRegistry` → `SAVE1.CAM` →
//!    `start_local_game_from_save`) lands on the same map and planet.
//! 3. [`mission_one_terrain_matches_the_interplay_planet_map`] — with the real
//!    game data: after the start-up planet's assets (`SNOW_1`) the mission
//!    start switches to `GREEN_5`, every one of the 12,544 terrain cells has
//!    the oracle `.WRL` tile id, and the tile palette is the oracle palette.
//!    The test skips itself when the game data is absent.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use max_app::local::LocalSetup;
use max_app::{App, AppConfig};
use max_assets::wrl::WrlMap;
use max_assets::MissionRegistry;
use max_core::enums::Planet;
use max_core::geo::{MAP_HEIGHT, MAP_TILES, MAP_WIDTH};
use max_core::victory::MissionCategory;
use max_render::terrain::palette_from_wrl;

/// `max-original/SAVE1.CAM` — campaign 1, "Islands in the Sun" (a copy).
const SAVE1: &[u8] = include_bytes!("data/save1_v70.cam");

/// V70 layout (`save.md`, `phase4_save.rs`):
/// `header (176) + IniOptions (12 × i32 = 48)`, then the raw surface map
/// (12,544 × u8) and the cargo map (12,544 × u16, little-endian).
const OFF_SURFACE_MAP: usize = 224;
const OFF_CARGO_MAP: usize = OFF_SURFACE_MAP + MAP_TILES;

/// Oracle planet of `SAVE1.CAM` (`world = 16`): Islandia, `GREEN_5.WRL`
/// (`WORLD_ASSETS`: `SNOW_1 + 16`).
const ORACLE_PLANET: Planet = Planet::Islandia;
const ORACLE_WRL: &str = "GREEN_5.WRL";

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// `PATCHES.RES` with the `SC_*` mission descriptors (stage-0 fixtures).
fn patches_res() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../max_assets/tests/data/PATCHES.RES")
}

/// A temporary data directory with `SAVE1.CAM` under its registry name.
fn mission_data_dir(name: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);

    let dir = std::env::temp_dir().join(format!(
        "max_app_mission_map_{name}_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("create temp data dir");
    std::fs::write(dir.join("SAVE1.CAM"), SAVE1).expect("SAVE1.CAM");
    dir
}

/// The oracle's surface map: the raw bytes of the save (`saveload.cpp:827`).
fn oracle_surface() -> &'static [u8] {
    &SAVE1[OFF_SURFACE_MAP..OFF_SURFACE_MAP + MAP_TILES]
}

/// The oracle's cargo map: the raw little-endian `uint16_t[12,544]`
/// (`saveload.cpp:829`).
fn oracle_cargo() -> &'static [u8] {
    &SAVE1[OFF_CARGO_MAP..OFF_CARGO_MAP + MAP_TILES * 2]
}

/// The map installed into the game state is exactly the save's data map
/// (`GameState::load_state` ← `SaveLoad_LoadFormatV70`), not the `.WRL` one.
fn assert_state_map_is_the_oracle_save(app: &App, context: &str) {
    assert_eq!(
        app.state_ref().map().surface.to_bytes().as_slice(),
        oracle_surface(),
        "{context}: the loaded surface map differs from SAVE1.CAM"
    );
    assert_eq!(
        app.state_ref().map().resources.to_le_bytes().as_slice(),
        oracle_cargo(),
        "{context}: the loaded cargo map differs from SAVE1.CAM"
    );
}

// ---------------------------------------------------------------------------
// 1. Headless V70 load: map bytes + header planet
// ---------------------------------------------------------------------------

#[test]
fn v70_load_installs_the_map_and_planet_of_the_header() {
    let mut app = App::new(AppConfig::default());
    app.load_save_bytes(SAVE1).expect("SAVE1.CAM loads");

    assert_eq!(
        app.state_ref().save_meta().world,
        ORACLE_PLANET,
        "the header's `world_index` (save.md `PlanetType`) is 16 = Islandia"
    );
    // No assets in a headless run: the loader still records the planet for a
    // later `init_assets` (`ResourceManager_InitInGameAssets(world_index)`).
    assert_eq!(
        app.config().planet,
        ORACLE_PLANET,
        "the loader must remember the planet of the save header"
    );
    assert_state_map_is_the_oracle_save(&app, "headless V70 load");
}

// ---------------------------------------------------------------------------
// 2. Mission start flow (registry → SAVE1.CAM → local match)
// ---------------------------------------------------------------------------

#[test]
fn mission_start_installs_the_planet_of_the_v70_header() {
    let dir = mission_data_dir("campaign1");
    let registry = MissionRegistry::open(patches_res(), &dir).expect("PATCHES.RES parses");

    let mut app = App::new(AppConfig::default());
    let start = app
        .start_mission_from_registry(
            &registry,
            MissionCategory::Campaign,
            0,
            &LocalSetup::default(),
        )
        .expect("campaign mission 1 starts");

    assert_eq!(start.tag, "SC_C0001");
    // `resolve_mission_file` tries the lower/upper spelling first (the data
    // directory is case-sensitive on Linux and the file is `SAVE1.CAM`).
    assert!(
        start
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("SAVE1.CAM")),
        "registry index 0 is SAVE1.CAM, got {}",
        start.path.display()
    );
    assert!(app.local_active(), "the mission is turn-active");
    assert_eq!(app.state_ref().save_meta().world, ORACLE_PLANET);
    assert_eq!(
        app.config().planet,
        ORACLE_PLANET,
        "the mission start must install the planet of the save header"
    );
    assert_state_map_is_the_oracle_save(&app, "campaign mission 1 start");
}

// ---------------------------------------------------------------------------
// 3. Real game data: terrain == the oracle planet map (GREEN_5.WRL)
// ---------------------------------------------------------------------------

#[test]
fn mission_one_terrain_matches_the_interplay_planet_map() {
    let data_dir = repo_path("max-original");
    let oracle_path = data_dir.join(ORACLE_WRL);

    assert!(data_dir.join("MAX.RES").is_file() && data_dir.join("COLOR.PAL").is_file() && oracle_path.is_file() && patches_res().is_file(), concat!("required test fixture is missing: ", stringify!(!data_dir.join("MAX.RES").is_file() || !data_dir.join("COLOR.PAL").is_file() || !oracle_path.is_file() || !patches_res().is_file())));

    // Start from the default planet (`SNOW_1`) exactly like the GUI does.
    let config = AppConfig::default()
        .with_data_dir(&data_dir)
        .with_assets_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../max_assets/tests/data"));
    let mut app = App::new(config);
    app.init_assets().expect("game data loads");
    assert_eq!(
        app.assets().expect("assets").planet,
        Planet::Snowcrab,
        "the application starts on the default planet"
    );

    let registry = MissionRegistry::open(patches_res(), &data_dir).expect("PATCHES.RES parses");
    let start = app
        .start_mission_from_registry(
            &registry,
            MissionCategory::Campaign,
            0,
            &LocalSetup::default(),
        )
        .expect("campaign mission 1 starts");

    assert_eq!(start.tag, "SC_C0001");
    assert_eq!(app.state_ref().save_meta().world, ORACLE_PLANET);
    // The defect: `config`/`assets` stayed on `SNOW_1` and the mission was drawn
    // with the wrong planet's terrain.
    assert_eq!(
        app.config().planet,
        ORACLE_PLANET,
        "the mission start must set `settings[\"world\"]` from the header"
    );
    let assets = app.assets().expect("assets stay loaded");
    assert_eq!(
        assets.planet, ORACLE_PLANET,
        "the terrain/minimap must come from the save's planet"
    );

    // The simulation map is still the save's data map.
    assert_state_map_is_the_oracle_save(&app, "campaign mission 1 with assets");

    // Every terrain cell must show the oracle planet's tile (`gfx.cpp:175`:
    // `World::GetTileIds()` of `SNOW_1 + world_index`).
    let oracle = WrlMap::open(&oracle_path).expect("GREEN_5.WRL parses");
    assert_eq!(oracle.width(), MAP_WIDTH);
    assert_eq!(oracle.height(), MAP_HEIGHT);
    assert_eq!(
        assets.terrain.tile_count(),
        oracle.tile_count(),
        "the mission uses the oracle planet's tileset"
    );

    let mut mismatches = 0usize;
    let mut first: Option<(i16, i16, Option<u16>, u16)> = None;
    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            let index = y as usize * MAP_WIDTH as usize + x as usize;
            let expected = oracle.tile_map()[index];
            let actual = assets.terrain.tile_id_at(x, y);
            if actual != Some(expected) {
                mismatches += 1;
                if first.is_none() {
                    first = Some((x, y, actual, expected));
                }
            }
        }
    }
    assert_eq!(
        mismatches, 0,
        "mission 1 terrain differs from {ORACLE_WRL} in {mismatches} cells (first: {first:?})"
    );

    // The palette is the oracle's as well (the tiles must be drawn with the
    // planet's colors, not only indexed by its tile ids).
    assert_eq!(
        assets.terrain.palette(),
        &palette_from_wrl(oracle.palette()),
        "the mission palette differs from {ORACLE_WRL}"
    );
}
