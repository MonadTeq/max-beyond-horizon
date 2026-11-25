use super::*;

/// Builds a minimal WRL of the given size (layer_count = 1).
fn build_wrl(
    width: i16,
    height: i16,
    tile_ids: &[u16],
    tile_count: u16,
    pass_table: &[u8],
) -> Vec<u8> {
    let cells = width as usize * height as usize;

    assert_eq!(tile_ids.len(), cells);
    assert_eq!(pass_table.len(), tile_count as usize);

    let mut out = Vec::new();
    out.extend_from_slice(&WRL_SIGNATURE);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend(std::iter::repeat_n(0u8, cells));

    for tile_id in tile_ids {
        out.extend_from_slice(&tile_id.to_le_bytes());
    }

    out.extend_from_slice(&tile_count.to_le_bytes());
    out.extend(std::iter::repeat_n(7u8, TILE_PIXELS * tile_count as usize));
    out.extend_from_slice(&[0u8; PALETTE_BYTES]);
    out.extend_from_slice(pass_table);
    out
}

#[test]
fn zero_tile_count_is_valid_and_yields_none_surface() {
    let bytes = build_wrl(112, 112, &[0u16; 12_544], 0, &[]);
    let map = WrlMap::parse(&bytes).unwrap();

    assert_eq!(map.tile_count(), 0);
    assert!(map.tiles().is_empty());
    assert_eq!(map.offsets().end, bytes.len());
    assert!(map.surface_map().to_bytes().iter().all(|&byte| byte == 0));
    assert!(map
        .warnings()
        .iter()
        .any(|warning| warning.contains("tile_count = 0")));
}

#[test]
fn out_of_range_values_become_none() {
    // 2×2: tile_id 5 is outside the set (tile_count = 1), pass 9 is outside 0..3.
    let bytes = build_wrl(2, 2, &[0, 5, 0, 0], 1, &[9]);
    let map = WrlMap::parse(&bytes).unwrap();

    assert_eq!(map.pass_surfaces(), vec![Surface::NONE]);
    assert_eq!(&map.surface_map().to_bytes()[..4], &[0u8; 4]);
    assert!(map
        .warnings()
        .iter()
        .any(|warning| warning.contains("tile_id")));
    assert!(map
        .warnings()
        .iter()
        .any(|warning| warning.contains("pass-таблицы")));

    // A valid pass 2 -> COAST.
    let bytes = build_wrl(2, 2, &[0, 5, 0, 0], 1, &[2]);
    let map = WrlMap::parse(&bytes).unwrap();

    assert_eq!(
        &map.surface_map().to_bytes()[..4],
        &[
            Surface::COAST.bits(),
            0,
            Surface::COAST.bits(),
            Surface::COAST.bits()
        ]
    );
}

#[test]
fn errors_are_returned_instead_of_panicking() {
    assert!(matches!(WrlMap::parse(b""), Err(WrlError::TooShort { .. })));
    assert!(matches!(
        WrlMap::parse(b"WRL\x01"),
        Err(WrlError::TooShort { .. })
    ));

    let mut bad_signature = build_wrl(2, 2, &[0; 4], 1, &[0]);
    bad_signature[0..3].copy_from_slice(b"XYZ");
    assert!(matches!(
        WrlMap::parse(&bad_signature),
        Err(WrlError::BadSignature { .. })
    ));

    let mut zero_layers = build_wrl(2, 2, &[0; 4], 1, &[0]);
    zero_layers[3..5].copy_from_slice(&0u16.to_le_bytes());
    assert!(matches!(
        WrlMap::parse(&zero_layers),
        Err(WrlError::ZeroLayers)
    ));

    let invalid_dims = {
        let mut bytes = build_wrl(2, 2, &[0; 4], 1, &[0]);
        bytes[5..7].copy_from_slice(&0i16.to_le_bytes());
        bytes
    };
    assert!(matches!(
        WrlMap::parse(&invalid_dims),
        Err(WrlError::InvalidDimensions { .. })
    ));
}

#[test]
fn dmo_signature_is_accepted_with_a_warning() {
    let mut bytes = build_wrl(2, 2, &[0; 4], 1, &[0]);
    bytes[0..3].copy_from_slice(&DMO_SIGNATURE);

    let map = WrlMap::parse(&bytes).unwrap();

    assert_eq!(map.signature(), DMO_SIGNATURE);
    assert!(map.warnings().iter().any(|warning| warning.contains("DMO")));
}
