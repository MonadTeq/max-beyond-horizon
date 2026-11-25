use super::*;

#[test]
fn cursor_ids_follow_the_cpp_enum_and_resource_table() {
    assert_eq!(CursorId::Hidden.index(), 0);
    assert_eq!(CursorId::Hand.resource_name(), "HANDPTR");
    assert_eq!(CursorId::Map.resource_name(), "MAP_PTR");
    assert_eq!(CursorId::Map2.resource_name(), "MAP_PTR");
    assert_eq!(CursorId::Path.resource_name(), "PTR_PATH");
    assert_eq!(CursorId::Help.index(), 29);
    assert_eq!(CursorId::from_index(30), None);

    for (index, id) in CursorId::ALL.into_iter().enumerate() {
        assert_eq!(id.index(), index);
        assert_eq!(id.resource_name(), CURSOR_RESOURCES[index]);
    }
}

#[test]
fn context_switches_cursor_like_cursor_setcursor() {
    assert_eq!(cursor_for_context(CursorContext::Menu), CursorId::Hand);
    assert_eq!(cursor_for_context(CursorContext::Panel), CursorId::Hand);
    assert_eq!(cursor_for_context(CursorContext::Minimap), CursorId::Hand);
    assert_eq!(cursor_for_context(CursorContext::MapIdle), CursorId::Map);
    assert_eq!(
        cursor_for_context(CursorContext::MapSelected(CursorHover::Friend)),
        CursorId::Friend
    );
    assert_eq!(
        cursor_for_context(CursorContext::MapSelected(CursorHover::Enemy)),
        CursorId::Enemy
    );
    assert_eq!(
        cursor_for_context(CursorContext::MapSelected(CursorHover::Empty)),
        CursorId::UnitGo
    );
    assert_eq!(
        cursor_for_context(CursorContext::Order { valid: false }),
        CursorId::UnitNoGo
    );
    assert_eq!(cursor_for_context(CursorContext::Help), CursorId::Help);
}

#[test]
fn session_cursor_switches_between_panel_and_map() {
    let app = crate::App::new(crate::AppConfig::default());

    assert_eq!(session_cursor(&app, None).0, CursorId::Hand);
    // Minimap (15,356) — HANDPTR, like the C++ default-window-cursor.
    assert_eq!(session_cursor(&app, Some((20, 360))).0, CursorId::Hand);
    // Map without selection — `CURSOR_UNIT_NO_GO`: `GameManager_GetWindowCursor`
    // (`game_manager.cpp:4305-4335`) has no `MAP_PTR` branch; `CURSOR_MAP`
    // remains only as a `Cursor_DefaultWindowCursorLut` record that
    // `GameManager_ProcessInput` always overrides.
    assert_eq!(
        session_cursor(&app, Some((300, 200))).0,
        CursorId::UnitNoGo
    );
}

#[test]
fn cursor_image_converts_palette_indices_and_hotspot() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original/MAX.RES");

    assert!(path.is_file(), concat!("required test fixture is missing: ", stringify!(!path.is_file())));

    let archive = max_assets::ResArchive::open(&path).expect("MAX.RES");
    let cursors = CursorSet::load(&archive);
    let mut palette = [[0u8, 0, 0, 255]; 256];

    for (index, entry) in palette.iter_mut().enumerate() {
        *entry = [index as u8, 1, 2, 255];
    }

    let hand = cursor_image(&cursors, &palette, CursorId::Hand, 0).expect("HANDPTR");
    assert_eq!((hand.width, hand.height), (18, 18));
    assert_eq!((hand.hot_x, hand.hot_y), (0, 1));
    assert_eq!(hand.rgba.len(), 18 * 18 * 4);

    let path_cursor = cursor_image(&cursors, &palette, CursorId::Path, 0).expect("PTR_PATH");
    assert_eq!((path_cursor.width, path_cursor.height), (23, 23));
    assert_eq!((path_cursor.hot_x, path_cursor.hot_y), (13, 13));
    assert_eq!(frame_count(&cursors, CursorId::Path), 10);
    assert_eq!(frame_count(&cursors, CursorId::Hand), 1);

    // The transparent index does not get into the frame as a color.
    assert!(frame_count(&cursors, CursorId::Hidden) == 1);
    assert!(cursor_image(&cursors, &palette, CursorId::Hidden, 1).is_none());
}
