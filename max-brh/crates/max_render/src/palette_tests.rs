use super::*;

/// Synthetic palette with a unique 6-bit color per index:
/// `(index % 64, (index / 64) * 16, 0)`.
fn synthetic_palette() -> Palette {
    let mut bytes = vec![0u8; PALETTE_BYTES];

    for index in 0..PALETTE_SIZE {
        bytes[index * 3] = (index % 64) as u8;
        bytes[index * 3 + 1] = ((index / 64) * 16) as u8;
        bytes[index * 3 + 2] = 0;
    }

    Palette::from_color_pal(&bytes).unwrap()
}

#[test]
fn six_bit_components_are_scaled_by_four() {
    let mut bytes = vec![0u8; PALETTE_BYTES];

    bytes[0] = 63;
    bytes[1] = 1;
    bytes[2] = 0;
    bytes[3] = 16;
    bytes[4] = 32;
    bytes[5] = 48;

    let palette = Palette::from_color_pal(&bytes).unwrap();

    assert_eq!(palette.raw(0), [63, 1, 0]);
    assert_eq!(palette.rgb(0), [252, 4, 0]);
    assert_eq!(palette.raw(1), [16, 32, 48]);
    assert_eq!(palette.rgb(1), [64, 128, 192]);
    assert_eq!(palette.rgba(1), Rgba::rgb(64, 128, 192));
    assert_eq!(palette.rgba_with_alpha(1, 7).a, 7);
    assert_eq!(palette.to_rgb_bytes().len(), PALETTE_BYTES);

    assert!(matches!(
        Palette::from_color_pal(&bytes[..PALETTE_BYTES - 1]),
        Err(PaletteError::TooShort { .. })
    ));
}

#[test]
fn team_luts_replace_command_slots_and_keep_the_rest() {
    let red = ColorLut::for_team(TeamColors::Red);
    let green = ColorLut::for_team(TeamColors::Green);
    let blue = ColorLut::for_team(TeamColors::Blue);
    let gray = ColorLut::for_team(TeamColors::Gray);
    let derelict = ColorLut::for_team(TeamColors::Derelict);

    assert!(!red.is_identity());
    assert!(ColorLut::identity().is_identity());

    // Slots 32–39 and 48–63 are substituted, everything else is the identity.
    for index in 0..PALETTE_SIZE {
        let in_slots = (32..=39).contains(&index) || (48..=63).contains(&index);

        if !in_slots {
            assert_eq!(red.map(index as u8), index as u8, "индекс {index}");
            assert_eq!(derelict.map(index as u8), index as u8, "индекс {index}");
        }
    }

    assert_eq!(&red.0[32..40], &TEAM_RED_COLORS);
    assert_eq!(&red.0[48..56], &TEAM_RED_COLORS);
    assert_eq!(&red.0[56..64], &TEAM_RED_COLORS);
    assert_eq!(&green.0[32..40], &TEAM_GREEN_COLORS);
    assert_eq!(&blue.0[48..56], &TEAM_BLUE_COLORS);
    assert_eq!(&gray.0[32..40], &TEAM_GRAY_COLORS);
    // Slots 48–63 = eight colors twice.
    assert_eq!(&derelict.0[48..56], &TEAM_DERELICT_COLORS);
    assert_eq!(&derelict.0[56..64], &TEAM_DERELICT_COLORS);
}

#[test]
fn command_slots_match_the_cpp_loop() {
    let slots = command_slots();

    assert_eq!(slots.len(), 24);
    assert_eq!(slots[0], (32, 0));
    assert_eq!(slots[7], (39, 7));
    assert_eq!(slots[8], (48, 0));
    assert_eq!(slots[15], (55, 7));
    assert_eq!(slots[16], (56, 0));
    assert_eq!(slots[23], (63, 7));

    // The same numbering as in the team LUT.
    let lut = ColorLut::for_team(TeamColors::Green);

    for (slot, color) in slots {
        assert_eq!(lut.map(slot), TEAM_GREEN_COLORS[color as usize]);
    }
}

#[test]
fn map_color_finds_exact_and_skips_reserved_ranges() {
    let palette = synthetic_palette();
    let value = palette.raw(40);

    assert_eq!(
        map_color(&palette.raw, value[0], value[1], value[2], false),
        40
    );
    assert_eq!(
        map_color(&palette.raw, value[0], value[1], value[2], true),
        40
    );

    // The colors of indices 9–31 and 96–127 are unreachable with full_scan = false.
    for reserved in [9u8, 20, 31, 96, 110, 127] {
        let value = palette.raw(reserved);
        let found = map_color(&palette.raw, value[0], value[1], value[2], false);

        assert!(
            !((9..=31).contains(&found) || (96..=127).contains(&found)),
                "map_color вернул зарезервированный индекс {found}"
        );
        // With full_scan the exact match is found.
        assert_eq!(
            map_color(&palette.raw, value[0], value[1], value[2], true),
            reserved
        );
    }
}

#[test]
fn brightness_levels_are_monotonic_and_level_seven_is_identity() {
    let palette = synthetic_palette();
    let brightness = BrightnessLut::build(&palette);

    for index in 0..PALETTE_SIZE {
        assert_eq!(
            brightness.map(FULL_BRIGHTNESS_LEVEL, index as u8),
            index as u8,
                "уровень 7 — identity"
        );
    }

    // Level 0 is the darkest: all colors except the fixed index 31
    // converge to black.
    for index in 0..PALETTE_SIZE {
        if index == usize::from(FIXED_BRIGHTNESS_INDEX) {
            continue;
        }

        let mapped = brightness.map(0, index as u8);

        assert_eq!(palette.raw(mapped), [0, 0, 0], "уровень 0, индекс {index}");
    }

    // Levels 0–6 are computed exactly as in `World::ApplyPaletteAndTints`:
    // rgb' = rgb * (level * 32) / 224, then `Color_MapColor` without full_scan.
    let raw = palette.raw_entries();

    for level in 0..7u32 {
        let factor = level * 32;

        for index in 0..PALETTE_SIZE {
            if index == usize::from(FIXED_BRIGHTNESS_INDEX) {
                continue;
            }

            let entry = raw[index];
            let expected = map_color(
                raw,
                (u32::from(entry[0]) * factor / (7 * 32)) as u8,
                (u32::from(entry[1]) * factor / (7 * 32)) as u8,
                (u32::from(entry[2]) * factor / (7 * 32)) as u8,
                false,
            );

            assert_eq!(
                brightness.map(level as u8, index as u8),
                expected,
                    "уровень {level}, индекс {index}"
            );
        }
    }

    // Level 5 (shadow) is noticeably darker: most colors change.
    let changed = (0..PALETTE_SIZE)
        .filter(|&index| brightness.map(SHADOW_BRIGHTNESS_LEVEL, index as u8) != index as u8)
        .count();

    assert!(changed > 100, "уровень 5 изменил всего {changed} цветов");

    // Index 31 is fixed at all levels 0–6.
    for level in 0..=6u8 {
        assert_eq!(brightness.map(level, FIXED_BRIGHTNESS_INDEX), 31);
    }

    // Levels are clamped.
    assert_eq!(brightness.level(200), brightness.level(7));
}

/// Palette where each slot is colored by its number: `(i, 2i, 4i)`.
fn indexed_palette() -> Palette {
    let mut bytes = vec![0u8; PALETTE_BYTES];

    for index in 0..PALETTE_SIZE {
        bytes[index * 3] = index as u8;
        bytes[index * 3 + 1] = (index * 2 % 64) as u8;
        bytes[index * 3 + 2] = (index * 4 % 64) as u8;
    }

    Palette::from_color_pal(&bytes).unwrap()
}

#[test]
fn color_cycle_table_matches_the_cpp_init_color_cycles() {
    // Order and timings — row by row `World::InitColorCycles`
    // (`world.cpp:542-557`); `TIMER_FPS_TO_MS(fps) = 1000 / fps`.
    let table: Vec<(u8, u8, RotateDirection, u32)> = COLOR_CYCLES
        .iter()
        .map(|cycle| {
            (
                cycle.start_index,
                cycle.end_index,
                cycle.direction,
                cycle.step_ms,
            )
        })
        .collect();

    assert_eq!(
        table,
        vec![
            (9, 12, RotateDirection::Left, 111),
            (13, 16, RotateDirection::Right, 166),
            (17, 20, RotateDirection::Right, 111),
            (21, 24, RotateDirection::Right, 166),
            (25, 30, RotateDirection::Right, 500),
            (31, 31, RotateDirection::Right, 166),
            (96, 102, RotateDirection::Right, 125),
            (103, 109, RotateDirection::Right, 125),
            (110, 116, RotateDirection::Right, 100),
            (117, 122, RotateDirection::Right, 166),
            (123, 127, RotateDirection::Right, 166),
        ]
    );

    // The mine beacons — 6 fps on four slots: full cycle 666.7 ms.
    let beacons = COLOR_CYCLES[1];

    assert_eq!((beacons.start_index, beacons.end_index), (13, 16));
    assert_eq!(beacons.len(), 4);
    assert_eq!(beacons.step_ms * beacons.len() as u32, 664);
}

#[test]
fn zero_dt_keeps_the_palette_identical() {
    let palette = indexed_palette();
    let mut cycles = ColorCycles::new();

    assert!(cycles.is_static());
    assert!(!cycles.advance(0.0));
    assert_eq!(rotate_palette(&palette, &cycles), palette);
    assert_eq!(cycles.step_counts(), &[0; COLOR_CYCLES.len()]);
}

#[test]
fn beacon_range_rotates_one_slot_per_sixth_of_a_second() {
    let palette = indexed_palette();
    let mut cycles = ColorCycles::new();

    // Less than the shortest period (10 fps = 100 ms) — not a single step.
    assert!(!cycles.advance(0.09));
    assert_eq!(rotate_palette(&palette, &cycles), palette);

    // 170 ms — exactly one 6 fps step (166 ms): 13..16 shifts right.
    assert!(cycles.advance(0.08));
    assert_eq!(cycles.step_counts()[1], 1);
    let once = rotate_palette(&palette, &cycles);

    assert_eq!(once.raw(13), palette.raw(16), "start <- end (вправо)");
    assert_eq!(once.raw(14), palette.raw(13));
    assert_eq!(once.raw(15), palette.raw(14));
    assert_eq!(once.raw(16), palette.raw(15));
    // The slots neighboring 13..16 are untouched (9..12 and 17..20 rotate by their own
    // 9 fps cycles, 13..16 do not depend on them).
    assert_eq!(once.raw(8), palette.raw(8));
    assert_eq!(once.raw(32), palette.raw(32));
    // 8-bit components are recomputed as raw * 4.
    assert_eq!(once.rgb(13), [once.raw(13)[0] * 4, once.raw(13)[1] * 4, once.raw(13)[2] * 4]);

    // A full cycle (4 steps of 1/6 s) returns slots 13..16 to their place
    // (the other ranges live at their own rate and move as well).
    for _ in 0..3 {
        cycles.advance(1.0 / 6.0);
    }
    assert_eq!(cycles.step_counts()[1], 4);
    let full = rotate_palette(&palette, &cycles);

    for index in 13..=16 {
        assert_eq!(full.raw(index), palette.raw(index), "слот {index}");
    }

    // The 5th step = 4 (a full circle) + 1: slots 13..16 as after one step.
    cycles.advance(1.0 / 6.0);
    assert_eq!(cycles.step_counts()[1], 5);
    let after_five = rotate_palette(&palette, &cycles);

    for index in 13..=16 {
        assert_eq!(after_five.raw(index), once.raw(index), "слот {index}");
    }
}

#[test]
fn each_cycle_uses_its_own_rate_and_direction() {
    let palette = indexed_palette();
    let mut cycles = ColorCycles::new();

    // 0.5 s: 9..12 @9 fps (4 steps = full circle), 25..30 @2 fps (1 step),
    // 110..116 @10 fps (5 steps = full circle).
    cycles.advance(0.5);
    let steps = *cycles.step_counts();

    assert_eq!(steps[0], 4, "9 fps: 111 мс × 4 = 444 мс");
    assert_eq!(steps[4], 1, "2 fps: 500 мс");
    assert_eq!(steps[8], 5, "10 fps: 100 мс × 5");
    assert_eq!(steps[1], 3, "6 fps: 166 мс × 3");
    assert_eq!(steps[5], 3, "одиночный слот 31 — тот же 6 fps");

    let rotated = rotate_palette(&palette, &cycles);

    // 9..12 — the only range rotating left; 4 steps = a circle.
    for index in 9..=12 {
        assert_eq!(rotated.raw(index), palette.raw(index));
    }
    // 25..30 @2 fps shifted one to the right.
    assert_eq!(rotated.raw(25), palette.raw(30));
    assert_eq!(rotated.raw(26), palette.raw(25));

    // Another step: 9..12 left by one slot.
    let mut left = ColorCycles::new();
    left.advance(1.0 / 9.0);
    let rotated = rotate_palette(&palette, &left);

    assert_eq!(rotated.raw(9), palette.raw(10), "влево: start <- next");
    assert_eq!(rotated.raw(12), palette.raw(9), "влево: end <- start");
}

#[test]
fn single_slot_31_walks_the_green_component() {
    assert_eq!(single_slot_value(0), 63);
    assert_eq!(single_slot_value(1), 31);
    assert_eq!(single_slot_value(2), 15);
    assert_eq!(single_slot_value(3), 7);
    assert_eq!(single_slot_value(4), 63);
    assert_eq!(single_slot_value(9), 31);

    let palette = indexed_palette();
    let mut cycles = ColorCycles::new();

    cycles.advance(1.0 / 6.0);
    let rotated = rotate_palette(&palette, &cycles);

    assert_eq!(rotated.raw(31)[1], 31, "зелёный канал 63 -> 31");
    assert_eq!(rotated.raw(31)[0], palette.raw(31)[0], "красный не меняется");
    assert_eq!(rotated.raw(31)[2], palette.raw(31)[2], "синий не меняется");
}

#[test]
fn from_rgb8_truncates_to_six_bit_and_roundtrips() {
    let mut bytes = vec![0u8; PALETTE_BYTES];

    bytes[0..3].copy_from_slice(&[255, 243, 159]);
    bytes[3..6].copy_from_slice(&[0, 64, 128]);

    let palette = Palette::from_rgb8(&bytes).unwrap();

    assert_eq!(palette.raw(0), [63, 60, 39]);
    assert_eq!(palette.rgb(0), [252, 240, 156]);
    assert_eq!(palette.raw(1), [0, 16, 32]);
    assert!(matches!(
        Palette::from_rgb8(&bytes[..PALETTE_BYTES - 1]),
        Err(PaletteError::TooShort { .. })
    ));
}
