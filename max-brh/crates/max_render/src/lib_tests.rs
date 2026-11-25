use super::*;

#[test]
fn pure_render_smoke() {
    // Palette of 768 bytes + rendering of one pixel into the buffer without SDL.
    let bytes = [7u8; PALETTE_BYTES];
    let palette = Palette::from_color_pal(&bytes).unwrap();
    let brightness = BrightnessLut::build(&palette);
    let renderer = SpriteRenderer::neutral(&palette, &brightness);
    let mut buffer = RgbaBuffer::new(1, 1);

    assert!(buffer.set(0, 0, renderer.color_for(0)));
    assert_eq!(buffer.get(0, 0), Some(Rgba::rgb(28, 28, 28)));

    let viewport = Viewport::new(320, 240);

    assert_eq!(viewport.tile_pixels(), DEFAULT_ZOOM as i32);
    assert!(viewport.visible_tiles().is_some());
    assert_eq!(draw_order(&[(0, 1), (0, 0)]), vec![1, 0]);
}
