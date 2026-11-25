use super::*;

/// C++ `UnitInfo::UnitInfo` (`unitinfo.cpp:276-294`): `brightness = 255`,
/// the remaining presentation fields are zero.
#[test]
fn presentation_defaults_match_cpp_unit_constructor() {
    let presentation = UnitPresentation::default();
    assert_eq!(presentation.brightness, u8::MAX);
    assert_eq!(presentation.shadow_offset, Point::ZERO);
    assert_eq!(presentation.sound, 0);
    assert_eq!(presentation.sprite_bounds, Rect::default());
    assert_eq!(presentation.shadow_bounds, Rect::default());
    assert_eq!(presentation.total_images, 0);
    assert_eq!(presentation.cursor, 0);
    assert!(!presentation.bobbed);
    assert_eq!(presentation, UnitPresentation::new());
}
