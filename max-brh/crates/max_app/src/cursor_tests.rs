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
    assert_eq!(cursor_for_context(CursorContext::MapIdle), CursorId::Hand);
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
    // Map without a selection — `HANDPTR` (owner rule). `GameManager_GetWindowCursor`
    // (`game_manager.cpp:4289-4302`) returns `CURSOR_HAND` for the rectangle drag; the
    // no-selection case follows the same hand instead of the `UNIT_NGO` arm of
    // `game_manager.cpp:4305-4335`. `CURSOR_MAP` is only a
    // `Cursor_DefaultWindowCursorLut` record that `GameManager_ProcessInput` always
    // overrides.
    assert_eq!(session_cursor(&app, Some((300, 200))).0, CursorId::Hand);
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

/// `Cursor_DrawAttackPowerCursorHelper` (`cursor.cpp:202-237`): a 35 px bar where
/// the first segment is the health left after the shot and the second one is the
/// damage the shot takes.
#[test]
fn the_attack_power_segments_follow_the_cpp_math() {
    assert_eq!(
        attack_power_segments(16, 6, 16),
        Some(AttackPower {
            remaining: 21,
            damage: 14
        }),
        "10 of 16 hits left: 10 * 35 / 16 = 21 px, the shot takes the rest"
    );
    assert_eq!(
        attack_power_segments(4, 9, 16),
        Some(AttackPower {
            remaining: 0,
            damage: 8
        }),
        "a lethal shot is clamped to the current hits (`:210-212`)"
    );
    assert_eq!(
        attack_power_segments(1, 5, 16),
        Some(AttackPower {
            remaining: 0,
            damage: 2
        })
    );
    assert_eq!(
        attack_power_segments(16, 6, 0),
        None,
        "no `ATTRIB_HITS`: C++ clears the bar area instead of drawing segments"
    );
}

/// `Cursor_DrawAttackPowerCursor` (`cursor.cpp:520-531`): the 37×5 frame and the
/// 35×3 interior at `(ulx - 18, uly + 15)` of the `CURSOR_ENEMY` art, in palette
/// indices (2 — remaining hits, 1 — damage, the cursor's transparent index — rest).
#[test]
fn the_enemy_cursor_carries_the_damage_bar() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../max-original/MAX.RES");

    assert!(path.is_file(), concat!("required test fixture is missing: ", stringify!(!path.is_file())));

    let archive = max_assets::ResArchive::open(&path).expect("MAX.RES");
    let cursors = CursorSet::load(&archive);
    let mut palette = [[0u8, 0, 0, 255]; 256];

    for (index, entry) in palette.iter_mut().enumerate() {
        *entry = [index as u8, 17, 34, 255];
    }

    let cursor = cursors.get(CursorId::Enemy.index()).expect("ENMY_PTR");
    assert_eq!((cursor.width, cursor.height), (37, 33));
    assert_eq!((cursor.hot_x, cursor.hot_y), (18, 13));

    let plain = cursor_image(&cursors, &palette, CursorId::Enemy, 0).expect("ENMY_PTR");
    let bar = AttackPower {
        remaining: 10,
        damage: 5,
    };
    let with_bar =
        cursor_image_with_attack_power(&cursors, &palette, CursorId::Enemy, 0, Some(bar))
            .expect("ENMY_PTR with the bar");

    let expected = |index: u8| -> [u8; 4] {
        if index == cursor.transparent {
            [0, 0, 0, 0]
        } else {
            let [r, g, b, _] = palette[usize::from(index)];
            [r, g, b, 255]
        }
    };
    let pixel = |image: &CursorImage, x: u32, y: u32| -> [u8; 4] {
        let offset = (y as usize * image.width as usize + x as usize) * 4;

        [
            image.rgba[offset],
            image.rgba[offset + 1],
            image.rgba[offset + 2],
            image.rgba[offset + 3],
        ]
    };

    // `lrx = ulx - 18 = 0`, `lry = uly + 15 = 28` — the bar sits on the bottom
    // edge of the 37×33 cursor, exactly like `draw_box(data, width, 0, 0, 36, 4)`.
    let (left, top) = (0u32, 28u32);

    assert_eq!(pixel(&with_bar, left, top), expected(0), "black frame, top-left");
    assert_eq!(
        pixel(&with_bar, left + 36, top + 4),
        expected(0),
        "black frame, bottom-right"
    );

    for row in 1..=3 {
        for column in 0..ATTACK_POWER_BAR_WIDTH {
            let index = if column < bar.remaining {
                2
            } else if column < bar.remaining + bar.damage {
                1
            } else {
                cursor.transparent
            };

            assert_eq!(
                pixel(&with_bar, left + 1 + column, top + row),
                expected(index),
                "bar pixel ({column}, {row})"
            );
        }
    }

    // The rest of the cursor art is untouched: no bar — no change.
    assert_eq!(pixel(&with_bar, 0, 0), pixel(&plain, 0, 0));
    assert_eq!(pixel(&with_bar, 36, 0), pixel(&plain, 36, 0));
    assert_eq!(cursor_image_with_attack_power(&cursors, &palette, CursorId::Enemy, 0, None), Some(plain));
}

/// `Cursor_DrawAttackPowerCursor` for a real unit pair: the bar shows
/// `UnitsManager_GetAttackDamage(selected, target, 0)` of the target's hits.
#[test]
fn the_attack_power_comes_from_the_attack_damage() {
    use max_core::team::TeamId;
    use max_core::unit::{UnitId, UnitInfo};

    let attacker = UnitInfo::new(UnitType::Tank, TeamId::RED, UnitId(1));
    let mut target = UnitInfo::new(UnitType::Tank, TeamId::GREEN, UnitId(2));
    // A freshly constructed `UnitInfo` has no hits; a deployed one gets
    // `ATTRIB_HITS` (`UnitsManager_DeployUnit`).
    target.hits = target
        .base_values
        .get_attribute(max_core::unit_values::ATTRIB_HITS)
        .max(1) as u16;
    let damage = max_core::combat::get_attack_damage(&attacker, &target, 0);
    let power = attack_power(&attacker, &target).expect("the tank has ATTRIB_HITS");

    assert_eq!(
        power,
        attack_power_segments(
            i32::from(target.hits),
            damage,
            i32::from(
                target
                    .base_values
                    .get_attribute(max_core::unit_values::ATTRIB_HITS)
            )
        )
        .expect("bar")
    );
    assert_eq!(
        power.remaining + power.damage,
        ATTACK_POWER_BAR_WIDTH,
        "an undamaged tank: the bar is split between the health left and the shot"
    );
    assert!(power.damage > 0, "an armed tank shows a damage segment");
}
