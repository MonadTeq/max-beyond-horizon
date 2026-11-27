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

#[test]
fn default_remaster_root_follows_the_workspace_layout() {
    let root = default_remaster_root(Some(Path::new("D:/dev/max-port-debug")));

    assert_eq!(
        root,
        Path::new("D:/dev/max-port-debug/max-brh/target/remaster")
    );
    // Without a repository root the path stays relative: `RemasterCache::open`
    // then sees an empty manifest and every layer falls back.
    assert_eq!(
        default_remaster_root(None),
        Path::new("target/remaster").to_path_buf()
    );
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
