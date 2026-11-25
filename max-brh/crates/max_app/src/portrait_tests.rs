use super::*;
use max_core::enums::UnitType;

fn frame(value: u8) -> FlicFrame {
    FlicFrame {
        width: 2,
        height: 1,
        pixels: vec![value; 2],
        palette: [[0; 3]; 256],
    }
}

fn animation(frames: usize) -> PortraitAnim {
    PortraitAnim {
        unit: UnitId::NO_ID,
        unit_type: UnitType::Tank,
        frames: (0..frames as u8).map(frame).collect(),
        elapsed: 0.0,
    }
}

#[test]
fn dt_zero_keeps_the_portrait_on_frame_zero() {
    let mut portrait = animation(30);

    // 600 frames with dt = 0 — like `local_update(0.0)` in QA.
    for _ in 0..600 {
        portrait.advance(0.0);
    }

    assert_eq!(portrait.elapsed(), 0.0);
    assert_eq!(portrait.frame_index(), 0);
    assert_eq!(portrait.frame().pixels, vec![0, 0]);
}

#[test]
fn fifteen_fps_advances_one_frame_per_sixty_seventh_of_a_second() {
    let mut portrait = animation(3);

    // 1/15 s — exactly the next frame (within float error of dt).
    portrait.advance(1.0 / 15.0);
    assert_eq!(portrait.frame_index(), 1);

    portrait.advance(1.0 / 15.0);
    assert_eq!(portrait.frame_index(), 2);

    // Wrapping: after the 3rd frame — 0 again.
    portrait.advance(1.0 / 15.0);
    assert_eq!(portrait.frame_index(), 0);

    // 60 frames of 1/60 s = one second = 15 animation frames (modulo 3).
    let mut portrait = animation(30);

    for _ in 0..60 {
        portrait.advance(1.0 / 60.0);
    }

    assert!(
        (portrait.elapsed() - 1.0).abs() < 1e-6,
            "{}",
        portrait.elapsed()
    );
    assert_eq!(portrait.frame_index(), 15);
}

#[test]
fn static_flic_never_advances() {
    let mut portrait = animation(1);

    portrait.advance(10.0);

    assert!(!portrait.is_animated());
    assert_eq!(portrait.elapsed(), 0.0);
    assert_eq!(portrait.frame_index(), 0);
}

#[test]
fn reset_returns_to_the_first_frame() {
    let mut portrait = animation(4);

    portrait.advance(0.5);
    assert_ne!(portrait.frame_index(), 0);

    portrait.reset();
    assert_eq!(portrait.frame_index(), 0);
}
