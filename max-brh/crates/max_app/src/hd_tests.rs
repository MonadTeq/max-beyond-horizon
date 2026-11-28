//! Unit tests of the HD presenter (`max_app::hd`, `remaster-hd.md` §R7).

use super::*;

#[test]
fn pixel_perfect_presenter_owns_no_hd_layer() {
    let presenter = HdPresenter::empty(RenderScale::PixelPerfect);

    assert!(!presenter.is_hd());
    assert_eq!(presenter.scale(), RenderScale::PixelPerfect);
    assert!(!presenter.has_any());
    assert!(!presenter.has_map_content());
    assert_eq!(presenter.bank_count(), 0);
    assert_eq!(presenter.vram_bytes(), 0);
}

#[test]
fn an_empty_hd_presenter_falls_back_for_everything() {
    let presenter = HdPresenter::empty(RenderScale::Hd2);

    assert!(presenter.is_hd());
    assert!(!presenter.has_any(), "no layer resolved a single asset");
    assert_eq!(presenter.stats(), HdStats::default());
    // Without an HD frame the caller keeps its own background: the historical
    // 1:1 chrome (or its nearest upscale) is the fallback.
    let mut frame = RgbaBuffer::new(4, 4);

    assert!(!presenter.draw_chrome_frame(&mut frame));
    assert_eq!(frame.get(0, 0), Some(max_render::buffer::Rgba::TRANSPARENT));
}

#[test]
fn stats_sum_every_layer_class() {
    let stats = HdStats {
        hd_tiles: 2,
        hd_sprites: 3,
        hd_shadows: 1,
        hd_chrome: 1,
        hd_fonts: 2,
        tiles_bytes: 100,
        sprites_bytes: 200,
        chrome_bytes: 300,
        fonts_bytes: 400,
        cache_bytes: 500,
    };

    assert_eq!(stats.total_bytes(), 1500);
    assert_eq!(stats.substituted(), 9);
    assert!((stats.total_mib() - 1500.0 / (1024.0 * 1024.0)).abs() < f64::EPSILON);
}

/// Unique temporary directory of the test.
fn temp_dir(tag: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "max_brh_hd_{tag}_{}_{stamp}",
        std::process::id()
    ));

    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// The portable catalog keeps the HD package next to the game; a development
/// checkout keeps the R2 artifact location under `max-brh/target/`.
#[test]
fn default_remaster_root_follows_the_game_catalog_layout() {
    let root = temp_dir("portable");
    std::fs::create_dir_all(root.join(REMASTER_DIR)).expect("remaster dir");

    assert_eq!(
        default_remaster_root(Some(&root)),
        root.join(REMASTER_DIR),
        "установленная раскладка: <каталог игры>/remaster"
    );

    // A development checkout has no catalog `remaster/`, but carries the
    // `max-brh/` workspace: the historical artifact path is kept.
    let dev = temp_dir("dev");
    std::fs::create_dir_all(dev.join("max-brh")).expect("workspace dir");

    assert_eq!(
        default_remaster_root(Some(&dev)),
        dev.join("max-brh/target/remaster")
    );

    // Without a game root the path stays relative: `RemasterCache::open` then
    // sees an empty manifest and every layer falls back.
    assert_eq!(
        default_remaster_root(None),
        Path::new("target/remaster").to_path_buf()
    );

    std::fs::remove_dir_all(root).ok();
    std::fs::remove_dir_all(dev).ok();
}

#[test]
fn layers_without_assets_keep_the_fallback_and_an_empty_budget() {
    let fonts = max_render::hd_text::HdFonts::empty(RenderScale::Hd4);
    let presenter = HdPresenter::with_layers(
        RenderScale::Hd4,
        None,
        max_render::hd_chrome::HdChrome::empty(RenderScale::Hd4),
        fonts,
    );

    assert!(presenter.is_hd());
    assert_eq!(presenter.scale(), RenderScale::Hd4);
    assert!(!presenter.has_any());
    assert_eq!(presenter.fonts().hd_count(), 0);
    assert_eq!(presenter.chrome().hd_count(), 0);
    assert_eq!(presenter.stats().total_bytes(), 0);
}
