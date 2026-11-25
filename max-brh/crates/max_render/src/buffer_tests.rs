use super::*;

#[test]
fn size_stride_and_transparent_default() {
    let buffer = RgbaBuffer::new(3, 2);

    assert_eq!(buffer.width(), 3);
    assert_eq!(buffer.height(), 2);
    assert_eq!(buffer.stride(), 12);
    assert_eq!(buffer.len(), 6);
    assert_eq!(buffer.pixels().len(), 24);
    assert_eq!(buffer.bounds(), PixelRect::new(0, 0, 3, 2));
    assert!(buffer.pixels().iter().all(|&byte| byte == 0));
    assert_eq!(buffer.get(0, 0), Some(Rgba::TRANSPARENT));
}

#[test]
fn set_get_and_out_of_bounds() {
    let mut buffer = RgbaBuffer::new(2, 2);
    let red = Rgba::rgb(255, 0, 0);

    assert!(buffer.set(1, 1, red));
    assert_eq!(buffer.get(1, 1), Some(red));
    assert!(!buffer.set(2, 1, red));
    assert!(!buffer.set(-1, 0, red));
    assert_eq!(buffer.get(2, 1), None);
    assert_eq!(buffer.get(-1, 0), None);

    // Pixel offset of (1, 1) = (1*2 + 1) * 4.
    assert_eq!(&buffer.pixels()[12..16], &[255, 0, 0, 255]);
}

#[test]
fn fill_rect_clips_to_buffer() {
    let mut buffer = RgbaBuffer::new(4, 4);
    let blue = Rgba::rgb(0, 0, 255);

    let painted = buffer.fill_rect(PixelRect::new(-2, -2, 4, 4), blue);

    assert_eq!(painted, 4);
    assert_eq!(buffer.get(0, 0), Some(blue));
    assert_eq!(buffer.get(1, 1), Some(blue));
    assert_eq!(buffer.get(2, 2), Some(Rgba::TRANSPARENT));

    assert_eq!(buffer.fill_rect(PixelRect::new(10, 10, 2, 2), blue), 0);
    assert_eq!(buffer.fill_rect(PixelRect::new(0, 0, 0, 0), blue), 0);
}

#[test]
fn rect_geometry() {
    let rect = PixelRect::new(10, 20, 30, 40);

    assert_eq!(rect.lrx(), 40);
    assert_eq!(rect.lry(), 60);
    assert!(rect.contains(10, 20));
    assert!(!rect.contains(40, 20));
    assert!(!rect.is_empty());
    assert!(PixelRect::new(0, 0, 0, 5).is_empty());

    let clipped = rect.intersect(PixelRect::new(0, 0, 20, 30)).unwrap();

    assert_eq!(clipped, PixelRect::from_corners(10, 20, 20, 30));
    assert!(rect.intersect(PixelRect::new(0, 0, 5, 5)).is_none());
}

#[test]
fn darken_and_mix_match_the_documented_formula() {
    let grey = Rgba::rgb(224, 112, 0);

    // level 7 (full brightness) changes nothing.
    assert_eq!(grey.darken(7), grey);
    // level 5 = 160/224 = 5/7 -> 160, 80, 0.
    assert_eq!(grey.darken(5), Rgba::rgb(160, 80, 0));
    // level 0 — black.
    assert_eq!(grey.darken(0), Rgba::rgb(0, 0, 0));
    // A level above 7 is clamped.
    assert_eq!(grey.darken(200), grey);
    // Alpha does not change.
    assert_eq!(Rgba::from_rgba([10, 20, 30, 0]).darken(3).a, 0);

    assert_eq!(Rgba::BLACK.mix(Rgba::WHITE, 255), Rgba::WHITE);
    assert_eq!(Rgba::BLACK.mix(Rgba::WHITE, 0), Rgba::BLACK);
    assert_eq!(Rgba::BLACK.mix(Rgba::WHITE, 128), Rgba::rgb(128, 128, 128));
}

#[test]
fn modify_changes_existing_pixels_only() {
    let mut buffer = RgbaBuffer::filled(2, 2, Rgba::WHITE);

    assert!(buffer.modify(0, 0, |color| color.darken(5)));
    assert_eq!(buffer.get(0, 0), Some(Rgba::rgb(182, 182, 182)));
    assert!(!buffer.modify(5, 5, |color| color.darken(5)));
    assert_eq!(buffer.get(1, 1), Some(Rgba::WHITE));
}
