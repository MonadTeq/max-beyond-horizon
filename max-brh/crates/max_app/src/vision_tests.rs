use super::*;

#[test]
fn cell_mask_sets_and_reads_cells() {
    let mut mask = CellMask::new();

    assert!(mask.is_empty());
    mask.set(5, 7);
    assert!(mask.get(5, 7));
    assert!(!mask.get(7, 5));
    // Out of the map: nothing is stored and nothing is reported.
    mask.set(-1, 0);
    mask.set(0, i32::from(max_core::geo::MAP_HEIGHT) + 5);
    assert!(!mask.get(-1, 0));
    assert!(!mask.get(0, 200));
    assert_eq!(mask.count(), 1);
}

#[test]
fn the_scan_coverage_is_a_disc_around_the_anchor() {
    let mut mask = CellMask::new();

    VisionView::add_scan(&mut mask, (10, 10), 2);

    // The anchor cell and the four neighbours are inside; a diagonal corner at
    // the square bound (2,2 cells away) is outside (the `* 64` euclidean check).
    assert!(mask.get(10, 10));
    assert!(mask.get(12, 10));
    assert!(mask.get(10, 12));
    assert!(!mask.get(12, 12));
    assert!(!mask.get(13, 10));
}
