use super::*;

/// 640×480 mini-frame: one color each, to check the blit.
fn fake_frame() -> BigImage {
    let width = max_render::layout::WINDOW_WIDTH as i16;
    let height = max_render::layout::WINDOW_HEIGHT as i16;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0i16.to_le_bytes());
    bytes.extend_from_slice(&0i16.to_le_bytes());
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());

    let mut palette = [0u8; max_assets::image::PALETTE_BYTES];
    palette[3..6].copy_from_slice(&[1, 2, 3]);
    bytes.extend_from_slice(&palette);

    // Each row: a literal of `width` bytes of index 1 (RLE with n > 0).
    let row = vec![1u8; width as usize];
    for _ in 0..height {
        bytes.extend_from_slice(&(width).to_le_bytes());
        bytes.extend_from_slice(&row);
    }

    BigImage::decode(&bytes).expect("мини-кадр должен разбираться")
}

#[test]
fn chrome_draws_frame_and_reports_ready() {
    let chrome = Chrome {
        frame: fake_frame(),
        panel_top: None,
        panel_bottom: None,
    };
    let mut frame = RgbaBuffer::new(640, 480);

    assert!(chrome.is_ready());
    assert_eq!(chrome.draw(&mut frame), 640 * 480);
    assert_eq!(frame.get(0, 0), Some(Rgba::rgb(1, 2, 3)));
    assert_eq!(frame.get(639, 479), Some(Rgba::rgb(1, 2, 3)));
}

#[test]
fn blit_map_lands_at_main_map_rect() {
    let mut frame = RgbaBuffer::new(640, 480);
    let mut map = RgbaBuffer::new(448, 448);
    map.set(0, 0, Rgba::rgb(9, 9, 9));
    map.set(447, 447, Rgba::rgb(7, 7, 7));

    assert_eq!(blit_map(&mut frame, &map), 448 * 448);
    assert_eq!(
        frame.get(layout::MAIN_MAP.ulx, layout::MAIN_MAP.uly),
        Some(Rgba::rgb(9, 9, 9))
    );
    assert_eq!(
        frame.get(layout::MAIN_MAP.lrx() - 1, layout::MAIN_MAP.lry() - 1),
        Some(Rgba::rgb(7, 7, 7))
    );
    assert_ne!(
        frame.get(layout::MAIN_MAP.ulx - 1, layout::MAIN_MAP.uly),
        Some(Rgba::rgb(9, 9, 9)),
            "за границей области карты буфер не тронут"
    );
}

#[test]
fn blit_map_clips_smaller_buffer() {
    let mut frame = RgbaBuffer::new(640, 480);
    let mut map = RgbaBuffer::new(32, 16);
    map.set(31, 15, Rgba::rgb(5, 5, 5));

    assert_eq!(blit_map(&mut frame, &map), 32 * 16);
    assert_eq!(
        frame.get(layout::MAIN_MAP.ulx + 31, layout::MAIN_MAP.uly + 15),
        Some(Rgba::rgb(5, 5, 5))
    );
}
