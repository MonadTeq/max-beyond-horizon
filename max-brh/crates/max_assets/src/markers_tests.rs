use super::*;

#[test]
fn real_max_res_has_all_resource_markers() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../max-original/MAX.RES");

    if !path.is_file() {
        return;
    }

    let archive = ResArchive::open(&path).expect("MAX.RES");
    let markers = ResourceMarkers::load(&archive);

    assert_eq!(
        markers.loaded(),
        3 * BIG_MARKER_COUNT + 3 * SMALL_MARKER_COUNT,
            "17 больших и 16 малых маркеров на каждый тип"
    );

    for kind in [ResourceKind::Raw, ResourceKind::Fuel, ResourceKind::Gold] {
        let big = markers.big(kind, 1).expect("большой маркер 1");
        assert_eq!((big.width, big.height), (62, 62));
        assert_eq!((big.ulx, big.uly), (31, 31), "hotspot в центре клетки");
        assert_eq!(
            Some(big.transparent),
            big.pixel(0, 0),
                "прозрачный индекс — первый пиксель картинки"
        );

        let empty = markers.big(kind, 0).expect("*MK0");
        assert_eq!((empty.width, empty.height), (38, 38));

        let small = markers.small(kind, 1).expect("малый маркер 1");
        assert_eq!((small.width, small.height), (14, 16));
        assert_eq!((small.ulx, small.uly), (7, 8));

        let icon = markers.icon(kind).expect("I_*");
        assert!(icon.width > 0 && icon.height > 0);
    }

    // An amount above 16 is clamped, like `min(material_type & CARGO_MASK, 16)`.
    assert_eq!(
        markers.big(ResourceKind::Raw, 31).map(|image| image.width),
        markers.big(ResourceKind::Raw, 16).map(|image| image.width)
    );
}
