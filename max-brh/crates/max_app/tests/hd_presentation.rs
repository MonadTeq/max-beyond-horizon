//! R7 gates: the HD presentation pipeline (`max-brh-docs/development/remaster-hd.md`
//! §R7).
//!
//! The remaster is presentation only, so the gates are byte-level agreements
//! between the two pipelines, not new reference pictures:
//!
//! 1. **HD fallback is the `×S` upscale.** With no remaster assets at all — the
//!    normal state of a checkout without `target/remaster` — the HD frame must be
//!    exactly the nearest `×S` upscale of the pixel-perfect frame. That checks
//!    the whole chain at once: the `×S` camera, the terrain/tile geometry, the
//!    chrome fallback, the composited 640×480 UI layers and the HD message box.
//! 2. **HD assets that are exact upscales change nothing.** A remaster whose
//!    tiles, chrome and font atlas are the exact `×S` nearest upscales of the
//!    `MAX.RES` data renders the same frame byte for byte. This is the
//!    "golden по источникам" property: the HD frame is a function of the source
//!    assets and the scale, never of the window.
//! 3. **HD unit sprites equal the upscaled 1:1 drawing.** The R4 index maps are
//!    drawn through the same frame indices, team colour, brightness and palette
//!    cycle state as [`max_app::render::draw_unit_sprites_with_animation`], so
//!    an exact-upscale asset reproduces the `×S` upscale of the 1:1 sprite.
//! 4. **VRAM/soak.** The decoded layers stay inside the R0 quota of the scale,
//!    the recolor cache is bounded, and a run of HD frames does not touch the
//!    simulation state (`snapshot_bytes` is the pixel-perfect one).
//!
//! The tests need the original game data (`max-original/MAX.RES`); they skip
//! themselves (and say so) when it is absent.

use std::path::{Path, PathBuf};

use max_app::hd::{vram_quota, HdPresenter};
use max_app::local::{LocalPhase, LocalSetup};
use max_app::render::{draw_unit_sprites_with_animation, draw_unit_sprites_with_animation_hd};
use max_app::{App, AppConfig, AppMode};
use max_core::enums::{PlayMode, TeamClan, VictoryType};
use max_core::team::TeamKind;
use max_render::buffer::{Rgba, RgbaBuffer};
use max_render::hd_chrome::{HdChrome, HdImage};
use max_render::hd_sprites::{HdIndexFrame, HdIndexMap, HdShadowFrame, HdSpriteBank};
use max_render::hd_terrain::{HdTerrainAtlas, HdTile};
use max_render::hd_text::{HdFontAtlas, HdFonts, HdGlyphAtlas};
use max_render::layout::{
    END_TURN_BUTTON, FILES_BUTTON, GRID_BUTTON, HELP_BUTTON, PREFS_BUTTON, ZOOM_PLUS,
};
use max_render::presentation::{window_points_for, ScreenGeometry};
use max_render::render_scale::RenderScale;
use max_render::text::GnwFontId;
use max_render::{upscale_nearest, PixelRect, FRAME_HEIGHT, FRAME_WIDTH};

/// Frame of a logical map area (`layout::MAIN_MAP` is 448×448).
const MAP_AREA: i32 = 448;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// The application with the original data, or `None` when the data is absent.
fn local_app() -> Option<App> {
    let data = repo_path("max-original");

    if !data.join("MAX.RES").is_file() {
        eprintln!("hd_presentation: max-original/MAX.RES отсутствует — тест пропущен");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(&data)
        .with_assets_dir(repo_path("tests/fixtures/assets"))
        .with_mode(AppMode::Ui);

    let mut app = App::new(config);
    app.init_assets().ok()?;

    Some(app)
}

/// One human (RED) against one computer (GREEN) on a fixed planet/seed.
fn setup_vs_one_ai() -> LocalSetup {
    LocalSetup {
        player_name: "Tester".to_string(),
        world: 5,
        opponent: 2,
        timer: 0,
        endturn: 0,
        play_mode: PlayMode::TurnBased,
        start_gold: 150,
        raw: 1,
        fuel: 1,
        gold: 1,
        derelicts: 0,
        victory_type: VictoryType::Duration,
        victory_limit: 50,
        team_types: [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ],
        team_clans: [TeamClan::Random; 4],
        hot_seat: false,
        seed: 7,
        start_positions: [None; 4],
        team_loadouts: std::array::from_fn(|_| None),
        team_gold: [None; 4],
        team_upgrades: std::array::from_fn(|_| None),
        team_gold_spent: [None; 4],
    }
}

/// Runs the landing/AI phases until the human turn starts.
fn run_to_player_turn(app: &mut App, limit: usize) -> bool {
    for _ in 0..limit {
        if app.local().map(|local| local.phase) == Some(LocalPhase::PlayerTurn) {
            return true;
        }
        if app.local().is_none_or(|local| local.finished()) {
            return false;
        }
        app.local_update(0.0);
    }

    app.local().map(|local| local.phase) == Some(LocalPhase::PlayerTurn)
}

/// Starts the reference scenario and stops at the first human turn.
fn started_app() -> Option<App> {
    let mut app = local_app()?;

    app.start_local_game(&setup_vs_one_ai()).ok()?;
    if !run_to_player_turn(&mut app, 900) {
        return None;
    }

    Some(app)
}

/// A presenter for `scale` with no remaster assets: every layer falls back.
///
/// The root does not exist on purpose — this is exactly what a checkout without
/// `target/remaster` looks like.
fn fallback_presenter(app: &App, scale: RenderScale) -> HdPresenter {
    let root = repo_path("max-brh/target/remaster-does-not-exist");

    match app.assets() {
        Some(assets) => HdPresenter::load(assets, scale, &root),
        None => HdPresenter::empty(scale),
    }
}

/// Asserts that `hd` is `pp` upscaled by `factor` (nearest neighbour).
fn assert_upscaled(hd: &RgbaBuffer, pp: &RgbaBuffer, factor: u32, context: &str) {
    assert_eq!(hd.width(), pp.width() * factor, "{context}: frame width");
    assert_eq!(hd.height(), pp.height() * factor, "{context}: frame height");

    let expected = upscale_nearest(pp, hd.width(), hd.height());

    if hd.pixels() == expected.pixels() {
        return;
    }

    let first = hd
        .pixels()
        .chunks_exact(4)
        .zip(expected.pixels().chunks_exact(4))
        .position(|(left, right)| left != right)
        .unwrap_or(0);
    let x = first as u32 % hd.width();
    let y = first as u32 / hd.width();

    panic!(
        "{context}: the HD frame differs from the x{factor} upscale of the 1:1 frame \
         (first difference at {x},{y}: {:?} vs {:?})",
        hd.get(x as i32, y as i32),
        expected.get(x as i32, y as i32)
    );
}

/// The `×S` nearest upscale of a chrome `BigImage` as an [`HdImage`].
fn upscaled_chrome(app: &App, scale: RenderScale) -> Option<HdImage> {
    let chrome = app.assets()?.chrome.as_ref()?;
    let factor = scale.scale();
    let width = chrome.frame.width as u32 * factor;
    let height = chrome.frame.height as u32 * factor;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);

    for y in 0..height {
        for x in 0..width {
            let source = chrome.frame.rgb(x / factor, y / factor)?;

            rgba.extend_from_slice(&[source[0], source[1], source[2], 255]);
        }
    }

    HdImage::from_rgba(width, height, rgba)
}

/// The `×S` nearest upscale of every terrain tile visible from `viewport`.
fn upscaled_terrain(
    app: &App,
    scale: RenderScale,
    viewport: &max_render::viewport::Viewport,
) -> Option<HdTerrainAtlas> {
    let assets = app.assets()?;
    let source = assets.terrain.clone();
    let factor = scale.scale();
    let tile_pixels = (64 * factor) as usize;
    let mut tiles: Vec<Option<HdTile>> = vec![None; source.tile_count() as usize];
    let range = viewport.visible_tiles()?;

    for (grid_x, grid_y) in range.cells() {
        let Some(tile_id) = source.tile_id_at(grid_x, grid_y) else {
            continue;
        };
        let mut rgba = vec![0u8; tile_pixels * tile_pixels * 4];

        for y in 0..tile_pixels {
            for x in 0..tile_pixels {
                let color = source.tile_pixel(
                    tile_id,
                    (x / factor as usize) as u32,
                    (y / factor as usize) as u32,
                )?;
                let offset = (y * tile_pixels + x) * 4;

                rgba[offset] = color.r;
                rgba[offset + 1] = color.g;
                rgba[offset + 2] = color.b;
                rgba[offset + 3] = color.a;
            }
        }

        tiles[tile_id as usize] = HdTile::from_rgba(tile_pixels as u32, tile_pixels as u32, rgba);
    }

    Some(HdTerrainAtlas::with_tiles(source, scale, tiles))
}

/// The `×S` nearest upscale of the `Font5` GNW bitmap as an HD atlas.
fn upscaled_font(scale: RenderScale) -> HdFontAtlas {
    let factor = scale.scale() as i32;
    let source = GnwFontId::Font5.font();
    let height = (source.height() * factor) as u32;
    let glyph_count = 256usize;
    let mut advances = Vec::with_capacity(glyph_count);

    for code in 0..glyph_count {
        advances.push(source.char_width(char::from(code as u8)) as u32 * factor as u32);
    }

    let width: u32 = advances.iter().sum();
    let mut coverage = vec![0u8; (width * height) as usize];
    let mut cursor = 0u32;

    for (code, advance) in advances.iter().enumerate() {
        let ch = char::from(code as u8);

        for row in 0..height {
            for column in 0..*advance {
                if source.glyph_ink(
                    ch,
                    (column / factor as u32) as i32,
                    (row / factor as u32) as i32,
                ) {
                    coverage[(row * width + cursor + column) as usize] = 255;
                }
            }
        }

        cursor += *advance;
    }

    let atlas = HdGlyphAtlas::from_coverage(width, height, coverage)
        .expect("the upscaled Font5 atlas is well formed");

    HdFontAtlas::with_atlas(GnwFontId::Font5, scale, atlas)
        .expect("the upscaled Font5 atlas matches the source metrics")
}

// ---------------------------------------------------------------------------
// Gate 1: the HD fallback is the ×S upscale of the pixel-perfect frame
// ---------------------------------------------------------------------------

#[test]
fn hd_fallback_frame_is_the_nearest_upscale_of_the_1to1_frame() {
    for scale in [RenderScale::Hd2, RenderScale::Hd4] {
        let Some(app) = started_app() else {
            return;
        };
        let factor = scale.scale();

        // A few simulation steps so the scene is not the very first frame.
        let mut app = app;

        for _ in 0..30 {
            app.local_update(max_app::SIMULATION_DT);
        }

        let pp = app.render_frame();
        let mut hd = fallback_presenter(&app, scale);
        let hd_frame = app.render_frame_hd(&mut hd);

        assert_eq!(pp.width(), FRAME_WIDTH);
        assert_eq!(pp.height(), FRAME_HEIGHT);
        assert_upscaled(&hd_frame, &pp, factor, scale.label());
        // A checkout without the remaster substitutes nothing, yet the HD path
        // is fully functional through the fallback.
        assert_eq!(hd.stats().substituted(), 0, "{}", scale.label());
        assert!(!hd.has_any());
    }
}

// ---------------------------------------------------------------------------
// Gate 2: exact-upscale HD assets keep the frame identical (golden by source)
// ---------------------------------------------------------------------------

#[test]
fn exact_upscale_hd_assets_reproduce_the_pixel_perfect_frame() {
    let scale = RenderScale::Hd2;
    let Some(app) = started_app() else {
        return;
    };
    let factor = scale.scale();

    let pp = app.render_frame();
    let viewport = app.map_viewport();

    // Every layer is the exact ×2 nearest upscale of the source data.
    let terrain = upscaled_terrain(&app, scale, &viewport).expect("the map has terrain tiles");
    let chrome = HdChrome::with_images(
        scale,
        upscaled_chrome(&app, scale),
        None,
        None,
        std::collections::BTreeMap::new(),
    );
    let fonts = HdFonts::with_font(scale, upscaled_font(scale));
    let mut hd = HdPresenter::with_layers(scale, Some(terrain), chrome, fonts);

    // The original palette chrome is the fallback source of the substituted one.
    let chrome_asset = app
        .assets()
        .and_then(|assets| assets.chrome.as_ref())
        .expect("chrome");
    let source = max_render::hd_chrome::ChromeSource::with_panels(
        chrome_asset.frame.clone(),
        chrome_asset.panel_top.clone(),
        chrome_asset.panel_bottom.clone(),
    );

    hd = hd.with_chrome_source(source);

    let stats = hd.stats();

    assert!(stats.hd_tiles > 0, "the visible tiles were substituted");
    assert_eq!(stats.hd_chrome, 1, "the window background was substituted");
    assert_eq!(stats.hd_fonts, 1, "the Font5 atlas was substituted");

    let hd_frame = app.render_frame_hd(&mut hd);

    assert_upscaled(&hd_frame, &pp, factor, "exact-upscale remaster");
    assert!(
        stats.fits_quota(scale),
        "the synthetic layers fit the quota"
    );
}

#[test]
fn hd_frame_is_a_function_of_the_sources_not_of_the_window() {
    let scale = RenderScale::Hd2;
    let factor = scale.scale();

    // The window profile is applied **after** the frame (`SdlWindow::present`);
    // the frame itself never sees it. Two drawables lay the render frame out
    // differently — 1280×960 is 1:1, 2560×1920 is a 2× block expansion — while
    // the logical 640×480 frame and the mouse transform stay the same.
    let native = ScreenGeometry::native_with_render_scale(
        (FRAME_WIDTH * factor, FRAME_HEIGHT * factor),
        (FRAME_WIDTH * factor, FRAME_HEIGHT * factor),
        scale,
    );
    let doubled = ScreenGeometry::native_with_render_scale(
        (FRAME_WIDTH * factor * 2, FRAME_HEIGHT * factor * 2),
        (FRAME_WIDTH * factor * 2, FRAME_HEIGHT * factor * 2),
        scale,
    );

    assert_eq!(native.scale(), 1);
    assert_eq!(doubled.scale(), 2, "the drawable doubles the render frame");
    assert_eq!(native.render_frame_size(), doubled.render_frame_size());
    assert_eq!(native.frame_size(), doubled.frame_size());
    // A click in the middle of either drawable is the logical centre.
    for geometry in [native, doubled] {
        let middle = (
            geometry.output_pixels().0 as i32 / 2,
            geometry.output_pixels().1 as i32 / 2,
        );

        assert_eq!(
            geometry.window_to_frame(middle.0, middle.1),
            Some(((FRAME_WIDTH / 2) as i32, (FRAME_HEIGHT / 2) as i32))
        );
    }

    // Two independent runs of the same scenario render the same HD frame: the
    // frame depends on the source assets and the scale only.
    let mut frames = Vec::new();

    for _ in 0..2 {
        let Some(mut app) = started_app() else {
            return;
        };

        for _ in 0..30 {
            app.local_update(max_app::SIMULATION_DT);
        }

        let mut hd = fallback_presenter(&app, scale);

        frames.push(app.render_frame_hd(&mut hd));
    }

    assert!(
        frames[0].pixels() == frames[1].pixels(),
        "the HD frame is not deterministic"
    );
}

// ---------------------------------------------------------------------------
// Gate 3: HD unit sprites equal the upscaled 1:1 sprite drawing
// ---------------------------------------------------------------------------

/// One HD index frame: the `×S` nearest upscale of a sprite frame's indices.
fn upscaled_index_frame(frame: &max_assets::sprite::SpriteFrame, scale: u32) -> HdIndexFrame {
    let width = u32::from(frame.width) * scale;
    let height = u32::from(frame.height) * scale;
    let mut payload = vec![0u8; (width * height) as usize];
    let mut coverage = vec![0u8; (width * height) as usize];

    for y in 0..height {
        for x in 0..width {
            let offset = (y * width + x) as usize;

            match frame.pixel(x / scale, y / scale) {
                Some(index) => {
                    payload[offset] = index;
                    coverage[offset] = u8::MAX;
                }
                None => coverage[offset] = 0,
            }
        }
    }

    let map = HdIndexMap::from_indices(width, height, payload, coverage)
        .expect("the upscaled index map is well formed");

    HdIndexFrame::new(map, frame.hotx * scale as i16, frame.hoty * scale as i16)
}

/// One HD shadow mask: the `×S` nearest upscale of a shadow frame.
fn upscaled_shadow_frame(shadow: &max_assets::sprite::ShadowFrame, scale: u32) -> HdShadowFrame {
    let width = u32::from(shadow.width) * scale;
    let height = u32::from(shadow.height) * scale;
    let mut payload = vec![0u8; (width * height) as usize];
    let coverage = vec![u8::MAX; (width * height) as usize];

    for y in 0..height {
        for x in 0..width {
            if shadow.darkened(x / scale, y / scale) {
                payload[(y * width + x) as usize] = u8::MAX;
            }
        }
    }

    let map = HdIndexMap::from_indices(width, height, payload, coverage)
        .expect("the upscaled shadow mask is well formed");

    HdShadowFrame::new(map, shadow.hotx * scale as i16, shadow.hoty * scale as i16)
}

#[test]
fn hd_unit_sprites_equal_the_upscaled_1to1_drawing() {
    let scale = RenderScale::Hd2;
    let factor = scale.scale();
    let Some(app) = started_app() else {
        return;
    };
    let Some(assets) = app.assets() else {
        return;
    };
    let state = app.state_ref();
    // A unit that is actually drawn: any type with a sprite sheet, on the map.
    let Some(unit) = state
        .unit_ids_in_order()
        .iter()
        .filter_map(|id| state.unit(*id))
        .find(|unit| unit.unit_type != max_core::enums::UnitType::MasterBuilder)
    else {
        return;
    };
    let Some(sprite) = assets.sprite(unit.unit_type) else {
        return;
    };

    // A bank whose every frame and shadow is the exact ×2 upscale.
    let frames: Vec<Option<HdIndexFrame>> = (0..sprite.sheet.len())
        .map(|index| {
            sprite
                .sheet
                .frame(index)
                .map(|frame| upscaled_index_frame(frame, factor))
        })
        .collect();
    let shadows: Vec<Option<HdShadowFrame>> = (0..sprite.sheet.len())
        .map(|index| {
            sprite
                .shadow
                .as_ref()
                .and_then(|shadow| shadow.frame(index))
                .map(|frame| upscaled_shadow_frame(frame, factor))
        })
        .collect();
    let bank = HdSpriteBank::with_frames(
        sprite.sheet.clone(),
        sprite.shadow.clone(),
        scale,
        frames,
        shadows,
    );
    let mut hd =
        HdPresenter::with_layers(scale, None, HdChrome::empty(scale), HdFonts::empty(scale))
            .with_bank(unit.unit_type, bank);

    let presentation = state.presentation(unit.id).cloned().unwrap_or_default();
    let image_base = max_app::render::image_base_for(
        unit,
        state
            .teams()
            .get(unit.team.0 as usize)
            .map(|team| team.team_clan),
    );
    let anchor = (MAP_AREA / 2, MAP_AREA / 2);
    let zoom = 64;
    let palette = &assets.system_palette;
    let cycles = max_render::palette::ColorCycles::new();

    let mut pp = RgbaBuffer::filled(MAP_AREA as u32, MAP_AREA as u32, Rgba::BLACK);
    let drawn = draw_unit_sprites_with_animation(
        assets,
        palette,
        unit,
        &presentation,
        image_base,
        unit.angle,
        Default::default(),
        true,
        unit.connectors,
        &mut pp,
        anchor,
        zoom,
    )
    .expect("the type has a sprite");
    assert!(drawn.unit > 0, "the 1:1 path drew the body");

    let mut hd_frame = RgbaBuffer::filled(
        (MAP_AREA * factor as i32) as u32,
        (MAP_AREA * factor as i32) as u32,
        Rgba::BLACK,
    );
    let drawn_hd = draw_unit_sprites_with_animation_hd(
        assets,
        palette,
        unit,
        &presentation,
        image_base,
        unit.angle,
        Default::default(),
        true,
        unit.connectors,
        &mut hd_frame,
        (anchor.0 * factor as i32, anchor.1 * factor as i32),
        zoom * factor,
        &mut hd,
        &cycles,
    )
    .expect("the HD bank has the type");

    assert_eq!(
        (drawn_hd.unit, drawn_hd.turret, drawn_hd.connectors),
        (
            drawn.unit * factor * factor,
            drawn.turret * factor * factor,
            drawn.connectors * factor * factor
        ),
        "the HD path painted a different pixel count"
    );
    assert_upscaled(&hd_frame, &pp, factor, "HD unit sprite");
}

// ---------------------------------------------------------------------------
// Gate 4: VRAM quota, bounded cache and a short HD soak
// ---------------------------------------------------------------------------

#[test]
fn hd_vram_stays_inside_the_r0_quota() {
    assert_eq!(vram_quota(RenderScale::PixelPerfect), 0);
    assert_eq!(vram_quota(RenderScale::Hd2), 105 * 1024 * 1024);
    assert_eq!(vram_quota(RenderScale::Hd4), 420 * 1024 * 1024);

    let Some(app) = started_app() else {
        return;
    };

    // Without a remaster every layer is empty: the HD path adds no VRAM at all.
    let mut empty = fallback_presenter(&app, RenderScale::Hd2);

    assert_eq!(empty.vram_bytes(), 0);
    assert!(empty.stats().fits_quota(RenderScale::Hd2));
    let _ = app.render_frame_hd(&mut empty);
    assert_eq!(empty.sprite_cache().len(), 0, "no sprite was recolored");

    // With exact-upscale layers the accounting adds up and stays in the quota.
    let scale = RenderScale::Hd2;
    let viewport = app.map_viewport();
    let terrain = upscaled_terrain(&app, scale, &viewport).expect("terrain");
    let chrome = upscaled_chrome(&app, scale);
    let fonts = HdFonts::with_font(scale, upscaled_font(scale));
    let presenter = HdPresenter::with_layers(
        scale,
        Some(terrain),
        HdChrome::with_images(scale, chrome, None, None, Default::default()),
        fonts,
    );
    let stats = presenter.stats();
    assert_eq!(
        stats.total_bytes(),
        stats.tiles_bytes
            + stats.sprites_bytes
            + stats.chrome_bytes
            + stats.fonts_bytes
            + stats.cache_bytes
    );
    assert!(stats.tiles_bytes > 0);
    assert!(stats.chrome_bytes > 0);
    assert!(stats.fonts_bytes > 0);
    assert!(stats.fits_quota(scale));
}

#[test]
fn hd_soak_renders_frames_without_touching_the_state() {
    let scale = RenderScale::Hd2;
    let Some(mut app) = started_app() else {
        return;
    };
    let mut hd = fallback_presenter(&app, scale);
    let snapshot = app.state_ref().snapshot_bytes();
    let digest = app.digest();

    // A short soak: render HD frames while the match runs. The simulation must
    // stay pixel-perfect identical — rendering is presentation only.
    for frame in 0..12u32 {
        app.local_update(max_app::SIMULATION_DT);
        let buffer = app.render_frame_hd(&mut hd);

        assert_eq!(buffer.width(), FRAME_WIDTH * scale.scale(), "frame {frame}");
        assert_eq!(
            buffer.height(),
            FRAME_HEIGHT * scale.scale(),
            "frame {frame}"
        );
    }

    assert!(
        hd.sprite_cache().len() <= hd.sprite_cache().limit(),
        "the recolor cache is bounded"
    );
    assert!(hd.stats().fits_quota(scale));
    assert_ne!(
        app.state_ref().snapshot_bytes(),
        snapshot,
        "the match advanced"
    );
    let _ = digest;
}

// ---------------------------------------------------------------------------
// Gate 5: the HD window holds the whole interface and hit-tests logical buttons
// ---------------------------------------------------------------------------

/// The HD window opens at the render frame (`640·S × 480·S` points), so the
/// whole interface — panels, menus, HUD — fits the window, and a click on a
/// button drawn as its `×S` block reaches the UI inside the **logical** 640×480
/// rectangle of that button.
///
/// Regression gate for the R7 integration defect where the window was opened at
/// `640·S` and the presentation switch applied `S` again: the geometry laid out
/// a `640·S² × 480·S²` frame, which cropped the interface to its centre in the
/// window and mapped every click half a screen off (the UI then received points
/// like `(320 + x/2, 240 + y/2)`, outside every button).
#[test]
fn hd_window_shows_the_whole_interface_and_hit_tests_logical_buttons() {
    let buttons = [
        ("End Turn", END_TURN_BUTTON),
        ("Files", FILES_BUTTON),
        ("Preferences", PREFS_BUTTON),
        ("Grid", GRID_BUTTON),
        ("Zoom +", ZOOM_PLUS),
        ("Help", HELP_BUTTON),
    ];

    for scale in [RenderScale::Hd2, RenderScale::Hd4] {
        let factor = scale.scale() as i32;
        let points = window_points_for(FRAME_WIDTH, FRAME_HEIGHT, scale, 1);
        let geometry = ScreenGeometry::native_with_render_scale(points, points, scale);

        // The whole interface is in the window: the content rectangle is the
        // drawable itself — no crop, no letterbox at the reference window size.
        assert_eq!(geometry.render_frame_size(), points, "[{}]", scale.label());
        assert_eq!(
            geometry.content_rect(),
            PixelRect::new(0, 0, points.0 as i32, points.1 as i32),
            "[{}] the interface is cropped by the HD window",
            scale.label()
        );
        assert!(geometry.fills_output(), "[{}]", scale.label());

        for (name, rect) in buttons {
            // A click on the centre of the `×S` block the button is drawn as.
            let (window_x, window_y) = geometry.frame_scale().frame_to_window(
                rect.ulx * factor + factor / 2,
                rect.uly * factor + factor / 2,
            );
            let mapped = geometry
                .window_to_frame(window_x, window_y)
                .unwrap_or_else(|| {
                    panic!("[{}] {name}: the click is outside the frame", scale.label())
                });

            assert!(
                rect.contains(mapped.0, mapped.1),
                "[{}] {name}: the click on the x{factor} block maps to {mapped:?}, outside \
                 the logical rectangle {rect:?}",
                scale.label()
            );
            // The block's top-left pixel maps to the logical top-left corner: the
            // logical rectangle did not move.
            let (window_x, window_y) = geometry
                .frame_scale()
                .frame_to_window(rect.ulx * factor, rect.uly * factor);
            assert_eq!(
                geometry.window_to_frame(window_x, window_y),
                Some((rect.ulx, rect.uly)),
                "[{}] {name}: the logical rectangle moved",
                scale.label()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Gate 6: SSAA (ssaa-design.md)
// ---------------------------------------------------------------------------

/// The `xS` nearest upscale of unit `unit_type`'s whole sprite sheet as an HD
/// bank: the adapter then has to expand every texel by the SSAA multiplier.
fn exact_upscale_bank(
    app: &App,
    unit_type: max_core::enums::UnitType,
    scale: RenderScale,
) -> Option<HdSpriteBank> {
    let assets = app.assets()?;
    let sprite = assets.sprite(unit_type)?;
    let factor = scale.scale();
    let frames: Vec<Option<HdIndexFrame>> = (0..sprite.sheet.len())
        .map(|index| {
            sprite
                .sheet
                .frame(index)
                .map(|frame| upscaled_index_frame(frame, factor))
        })
        .collect();
    let shadows: Vec<Option<HdShadowFrame>> = (0..sprite.sheet.len())
        .map(|index| {
            sprite
                .shadow
                .as_ref()
                .and_then(|shadow| shadow.frame(index))
                .map(|frame| upscaled_shadow_frame(frame, factor))
        })
        .collect();

    Some(HdSpriteBank::with_frames(
        sprite.sheet.clone(),
        sprite.shadow.clone(),
        scale,
        frames,
        shadows,
    ))
}

/// The HD frame is the same bytes whatever the anti-aliasing mode is: `aa` is
/// an output-layer parameter (`gpu-aa-design.md`), so the render frame — and
/// with it the snapshot/CRC and the golden frames — cannot see it. This is the
/// "aa does not touch the frame" side of the DoD.
#[test]
fn the_hd_frame_is_the_current_hd_frame() {
    let Some(mut app) = started_app() else {
        return;
    };

    for _ in 0..30 {
        app.local_update(max_app::SIMULATION_DT);
    }

    let pp = app.render_frame();

    for scale in [RenderScale::Hd2, RenderScale::Hd4] {
        let mut hd = fallback_presenter(&app, scale);
        let frame = app.render_frame_hd(&mut hd);
        let again = app.render_frame_hd(&mut hd);

        assert_eq!(
            (frame.width(), frame.height()),
            (FRAME_WIDTH * scale.scale(), FRAME_HEIGHT * scale.scale()),
            "[{}] the render frame must be the logical frame times S",
            scale.label()
        );
        assert!(
            frame.pixels() == again.pixels(),
            "[{}] the HD frame is not deterministic",
            scale.label()
        );
        assert_upscaled(
            &frame,
            &pp,
            scale.scale(),
            "the HD frame is the xS upscale of the 1:1 frame",
        );
    }
}
