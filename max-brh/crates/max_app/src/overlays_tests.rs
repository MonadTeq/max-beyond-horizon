use super::*;

#[test]
fn grid_colors_follow_the_world_overlay_table() {
    // `world.cpp:536-539`: RGB / 4 (integer), then `Color_MapColor`.
    assert_eq!(GRID_RGB_LAND, [12, 24, 2]);
    assert_eq!(GRID_RGB_WATER, [14, 20, 43]);
    assert_eq!(GRID_RGB_COAST, [20, 39, 5]);
    assert_eq!(GRID_RGB_AIR, [51, 8, 0]);
}

#[test]
fn circle_is_drawn_on_the_border_only() {
    let mut frame = RgbaBuffer::filled(64, 64, Rgba::BLACK);
    let color = Rgba::from_rgb([255, 0, 0]);
    let painted = draw_circle(&mut frame, 32, 32, 10, color);

    assert!(painted > 0, "круг нарисован");
    assert_eq!(frame.get(32, 22), Some(color), "верхняя точка радиуса");
    assert_eq!(frame.get(32, 42), Some(color), "нижняя точка радиуса");
    assert_eq!(frame.get(22, 32), Some(color), "левая точка радиуса");
    assert_eq!(frame.get(42, 32), Some(color), "правая точка радиуса");
    assert_eq!(frame.get(32, 32), Some(Rgba::BLACK), "внутри пусто");
}

#[test]
fn bar_display_fills_inclusive_rectangle() {
    let mut frame = RgbaBuffer::filled(64, 64, Rgba::BLACK);
    let color = Rgba::from_rgb([1, 2, 3]);
    let painted = fill_bar(&mut frame, 10, 10, 20, 12, color);

    assert_eq!(painted, 11 * 3);
    assert_eq!(frame.get(10, 10), Some(color));
    assert_eq!(frame.get(20, 12), Some(color));
    assert_eq!(frame.get(21, 10), Some(Rgba::BLACK));
}
