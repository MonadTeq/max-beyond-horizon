use super::*;

/// A palette in which the text (`0xA2`) and red (`0x01`) are distinguishable from the background.
fn test_palette() -> UiPalette {
    let mut colors = [[0u8, 0, 0]; 256];
    colors[usize::from(TEXT)] = [252, 248, 244];
    colors[usize::from(RED)] = [252, 0, 0];
    colors[usize::from(END_TURN_TEXT)] = [252, 248, 244];
    colors[2usize] = [0, 252, 0];
    colors[usize::from(TEAM_TEXT_SHADOW)] = [36, 12, 4];

    UiPalette::from_colors(colors)
}

/// "Ink" bounds of the color `color` inside the rectangle (inclusive).
fn ink_bounds(
    frame: &RgbaBuffer,
    rect: PixelRect,
    color: Rgba,
) -> Option<(i32, i32, i32, i32)> {
    let mut bounds: Option<(i32, i32, i32, i32)> = None;

    for y in rect.uly..rect.lry() {
        for x in rect.ulx..rect.lrx() {
            if frame.get(x, y) != Some(color) {
                continue;
            }

            bounds = Some(match bounds {
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                None => (x, y, x, y),
            });
        }
    }

    bounds
}

/// `GameManager_DrawTurnTimer`: `sprintf(text, "%2.2i:%2.2i", t / 60, t % 60)`
/// — leading zeros, minutes:seconds (`%2.2i` = width 2, precision 2).
#[test]
fn timer_text_matches_cpp_printf() {
    assert_eq!(timer_text(0), "00:00");
    assert_eq!(timer_text(1), "00:01");
    assert_eq!(timer_text(20), "00:20");
    assert_eq!(timer_text(21), "00:21");
    assert_eq!(timer_text(45), "00:45");
    assert_eq!(timer_text(101), "01:41");
    assert_eq!(timer_text(180), "03:00");
    assert_eq!(timer_text(600), "10:00");
    assert_eq!(timer_text(3599), "59:59");
}

/// Timer color: `turn_time <= 20` → `COLOR_RED`, otherwise `0xA2`
/// (`game_manager.cpp:2197-2213`). Zero is red too.
#[test]
fn timer_is_red_at_twenty_seconds_or_less() {
    assert_eq!(timer_color_index(0), RED);
    assert_eq!(timer_color_index(1), RED);
    assert_eq!(timer_color_index(20), RED);
    assert_eq!(timer_color_index(21), TEXT);
    assert_eq!(timer_color_index(45), TEXT);
    assert_eq!(timer_color_index(180), TEXT);
}

/// Timer text geometry in the `WINDOW_TURN_TIMER_DISPLAY` window
/// (`windows[]`: 534,2…594,22) — checked against the DOS original frame
/// (`build/anim-dos/avicap/f1/f_0001.png`, timer «01:41»): the ink
/// occupies x 551…575, y 9…15.
#[test]
fn timer_text_geometry_matches_the_dos_frame() {
    let palette = test_palette();
    let mut frame = RgbaBuffer::filled(640, 480, Rgba::from_rgb([0, 0, 0]));
    let font = GnwFontId::Font5.font();

    draw_centered(
        &mut frame,
        layout::TURN_TIMER,
        font,
        &timer_text(101),
        palette.rgba(TEXT),
    );

    let bounds = ink_bounds(&frame, layout::TURN_TIMER, palette.rgba(TEXT))
        .expect("текст «01:41» нарисован");

    assert_eq!(
        bounds,
        (551, 9, 575, 15),
            "таймер «01:41» обязан стоять как в оригинале"
    );
}

/// The End Turn button is pressed only on the computer turn.
#[test]
fn end_turn_is_pressed_only_on_a_computer_turn() {
    assert!(end_turn_pressed(TeamKind::Computer));
    assert!(!end_turn_pressed(TeamKind::Player));
    assert!(!end_turn_pressed(TeamKind::Remote));
    assert!(!end_turn_pressed(TeamKind::None));
    assert!(!end_turn_pressed(TeamKind::Eliminated));
}

/// Label color of the pressed button — `FontColor(team + 1, team + 1, 0x3F)`,
/// for the gray team `0xFF` (`game_manager.cpp:7876-7882`).
#[test]
fn end_turn_caption_uses_the_active_team_color() {
    assert_eq!(end_turn_caption_color(TeamId::RED), 1);
    assert_eq!(end_turn_caption_color(TeamId::GREEN), 2);
    assert_eq!(end_turn_caption_color(TeamId::BLUE), 3);
    assert_eq!(end_turn_caption_color(TeamId::GRAY), GRAY_TEAM_TEXT);
}

#[test]
fn caption_offsets_match_the_cpp_label_table() {
    // `GameManager_MenuItemLabelOffsets` (`game_manager.cpp:275-278`).
    assert_eq!(PanelButton::Files.caption_offset(), (0, 2));
    assert_eq!(PanelButton::Preferences.caption_offset(), (0, 2));
    assert_eq!(PanelButton::Prev.caption_offset(), (-10, 2));
    assert_eq!(PanelButton::Next.caption_offset(), (10, 2));
    assert_eq!(PanelButton::Done.caption_offset(), (0, 3));
    assert_eq!(PanelButton::Reports.caption_offset(), (0, 3));
    assert_eq!(PanelButton::Chat.caption_offset(), (0, 3));
    assert_eq!(PanelButton::Survey.caption_offset(), (2, 2));
    assert_eq!(PanelButton::Status.caption_offset(), (2, 2));
    assert_eq!(PanelButton::Colors.caption_offset(), (2, 4));
    assert_eq!(PanelButton::Hits.caption_offset(), (2, 2));
    assert_eq!(PanelButton::Ammo.caption_offset(), (2, 2));
    assert_eq!(PanelButton::Range.caption_offset(), (2, 4));
    assert_eq!(PanelButton::Scan.caption_offset(), (2, 2));
    assert_eq!(PanelButton::Grid.caption_offset(), (2, 2));
    assert_eq!(PanelButton::Names.caption_offset(), (2, 4));
    assert_eq!(PanelButton::EndTurn.caption_offset(), (0, 0));
}

#[test]
fn captions_are_centered_inside_the_inset_button_rect() {
    // `Button::SetCaption` (`button.cpp:412-449`) + `Text_TextLine`
    // (`text.cpp:772`): centering inside `width - x` / `height - y`.
    for button in PanelButton::ALL {
        let Some(caption) = button.caption() else {
            continue;
        };
        let rect = button.rect();
        let font = button.caption_font().font();
        let (offset_x, offset_y) = button.caption_offset();
        let (x, y) = button.caption_position();
        let text_width = font.text_width(caption) + 1;

        assert!(
            x >= rect.ulx && x + text_width <= rect.lrx(),
                "{button:?}: подпись «{caption}» вылезает за кнопку"
        );
        assert_eq!(
            y,
            rect.uly + (rect.height - offset_y - font.height()) / 2 + offset_y,
                "{button:?}: вертикальная центровка 1:1 button.cpp:433"
        );

        let expected_x = rect.ulx + ((rect.width - offset_x - text_width) / 2).max(0);
        assert_eq!(x, expected_x, "{button:?}: горизонтальная центровка");
    }

    // Exact values for the top 3×3 row (`FONT_2` — 9 px, height 16):
    // C++ `((16 - 2) - 9) / 2 + 2 = 4`.
    assert_eq!(PanelButton::Survey.caption_position().1, 298 + 4);
    assert_eq!(PanelButton::Hits.caption_position().1, 298 + 4);
    // COLORS/RANGE/NAMES: `((18 - 4) - 9) / 2 + 4 = 6`.
    assert_eq!(PanelButton::Colors.caption_position().1, 330 + 6);
    assert_eq!(PanelButton::Names.caption_position().1, 330 + 6);
}

#[test]
fn panel_buttons_have_the_original_window_rects() {
    assert_eq!(PanelButton::Files.rect(), PixelRect::new(17, 4, 67, 18));
    assert_eq!(PanelButton::EndTurn.rect(), PixelRect::new(390, 2, 72, 21));
    assert_eq!(
        PanelButton::ZoomSlider.rect(),
        PixelRect::new(20, 275, 130, 14)
    );
    assert_eq!(PanelButton::Survey.rect(), PixelRect::new(4, 298, 55, 16));
    assert_eq!(PanelButton::Names.rect(), PixelRect::new(114, 330, 55, 18));
}

#[test]
fn panel_hit_testing_covers_each_button_and_ignores_the_map() {
    for button in PanelButton::ALL {
        let rect = button.rect();
        assert_eq!(
            panel_button_at(rect.ulx, rect.uly),
            Some(button),
                "{button:?}: левый верх"
        );
        assert_eq!(
            panel_button_at(rect.lrx() - 1, rect.lry() - 1),
            Some(button),
                "{button:?}: правый низ"
        );
    }

    assert_eq!(panel_button_at(300, 200), None, "центр карты — не кнопка");
}

#[test]
fn end_turn_overlaps_the_map_but_wins_the_hit_test() {
    assert!(layout::END_TURN_BUTTON
        .intersect(layout::MAIN_MAP)
        .is_some());
    assert_eq!(panel_button_at(400, 10), Some(PanelButton::EndTurn));
    assert_eq!(screen_to_map(400, 10), None, "кнопка перекрывает карту");
}

#[test]
fn screen_to_map_subtracts_the_map_origin() {
    assert_eq!(screen_to_map(180, 18), Some((0, 0)));
    assert_eq!(screen_to_map(627, 465), Some((447, 447)));
    assert_eq!(screen_to_map(179, 18), None);
    assert_eq!(screen_to_map(180, 17), None);
    assert_eq!(screen_to_map(628, 465), None);
}

#[test]
fn toggles_are_local_flags() {
    let mut panel = PanelState::default();

    assert!(!panel.get(PanelButton::Grid));
    assert!(panel.toggle(PanelButton::Grid));
    assert!(panel.get(PanelButton::Grid));
    assert!(!panel.toggle(PanelButton::Grid));
    assert!(!panel.toggle(PanelButton::EndTurn), "не переключатель");
}

#[test]
fn minimap_unit_dots_respect_fov() {
    use max_core::enums::UnitType;
    use max_core::geo::{Point, MAP_TILES};
    use max_core::team::TeamId;

    let mut state = max_core::universe::GameUniverse::default();
    let own = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
        .unwrap();
    let enemy = state
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(20, 20), 0)
        .unwrap();
    let mut layer = vec![0u8; MAP_TILES];

    paint_minimap_units(
        &mut layer,
        StateRef::from(&state),
        112,
        112,
        false,
        false,
        false,
        None,
    );
    assert_eq!(
        layer[10 * 112 + 10],
        minimap_unit_color(TeamId::RED),
            "свой юнит виден"
    );
    assert_eq!(layer[20 * 112 + 20], 0, "чужой вне FOV не рисуется");

    // Surveyed (got into the FOV/radar) — the dot appears.
    state
        .unit_mut(enemy)
        .unwrap()
        .set_visible_to_team(TeamId::RED, true);
    layer.fill(0);
    paint_minimap_units(
        &mut layer,
        StateRef::from(&state),
        112,
        112,
        false,
        false,
        false,
        None,
    );
    assert_eq!(
        layer[20 * 112 + 20],
        minimap_unit_color(TeamId::GREEN),
            "видимый чужой юнит появляется на миникарте"
    );
    assert_eq!(layer[10 * 112 + 10], minimap_unit_color(TeamId::RED));
    let _ = own;
}

#[test]
fn minimap_unit_dots_use_the_no_fow_debug_mode() {
    use max_core::enums::UnitType;
    use max_core::geo::{Point, MAP_TILES};
    use max_core::team::TeamId;

    let mut state = max_core::universe::GameUniverse::default();
    let _own = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
        .unwrap();
    let _enemy = state
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(20, 20), 0)
        .unwrap();
    let mut layer = vec![0u8; MAP_TILES];

    // Debug `--no-fow`: the enemy that is outside the FOV is drawn too.
    paint_minimap_units(
        &mut layer,
        StateRef::from(&state),
        112,
        112,
        false,
        false,
        true,
        None,
    );

    assert_eq!(
        layer[10 * 112 + 10],
        minimap_unit_color(TeamId::RED),
            "own unit is drawn"
    );
    assert_eq!(
        layer[20 * 112 + 20],
        minimap_unit_color(TeamId::GREEN),
            "the debug mode draws the enemy without the fog of war"
    );
}

#[test]
fn zoom_slider_maps_to_viewport_zoom() {
    assert_eq!(zoom_slider_offset(64), 14);
    assert_eq!(zoom_slider_offset(8), 116);
    assert!(zoom_slider_offset(8) > zoom_slider_offset(64));

    assert_eq!(zoom_from_slider_x(20), 64, "левый край — 1:1");
    assert_eq!(zoom_from_slider_x(149), 8, "правый край — мелкий");
    assert!(
        zoom_from_slider_x(20 + 14 + 51) < 64,
            "середина — между краями"
    );

    for zoom in [8u32, 16, 32, 48, 64] {
        let x = layout::ZOOM_SLIDER.ulx + zoom_slider_offset(zoom);
        let roundtrip = zoom_from_slider_x(x);

        assert!(
            roundtrip.abs_diff(zoom) <= 2,
                "zoom {zoom} -> x {x} -> {roundtrip}"
        );
    }
}

#[test]
fn map_viewport_is_448_and_keeps_camera() {
    let mut viewport = Viewport::new(640, 480);
    viewport.center_on_grid(50, 50);

    let map = map_viewport(&viewport);

    assert_eq!(map.screen(), (448, 448));
    assert_eq!(map.center(), viewport.center());
    assert_eq!(map.zoom(), viewport.zoom());
    assert_eq!(map.screen_to_grid(224, 224), (50, 50));
}

#[test]
fn captions_and_fonts_match_game_manager() {
    assert_eq!(PanelButton::Files.caption(), Some("Files"));
    assert_eq!(PanelButton::EndTurn.caption(), Some("End Turn"));
    assert_eq!(PanelButton::EndTurn.caption_font(), GnwFontId::Font5);
    assert_eq!(PanelButton::Survey.caption_font(), GnwFontId::Font2);
    assert_eq!(PanelButton::Names.caption(), Some("Names"));
    assert_eq!(PanelButton::ZoomPlus.caption(), None);
}

#[test]
fn unit_name_falls_back_to_the_type() {
    use max_core::enums::UnitType;
    use max_core::unit::UnitInfo;

    let mut unit = UnitInfo::new(UnitType::Tank, max_core::TeamId::RED, max_core::UnitId(1));
    assert_eq!(unit_name(&unit), "Tank");
    unit.name = "AVENGER".to_string();
    assert_eq!(unit_name(&unit), "AVENGER");
}

/// The panels load from the real `MAX.RES`, and every zone changes pixels.
#[test]
fn minimap_view_matches_calculate_minimap_scaling() {
    let view = minimap_view(112, 112);
    assert_eq!((view.offset_x, view.offset_y), (0, 0));
    assert_eq!((view.width, view.height), (112, 112));
    assert_eq!(view.scale, 1.0);

    // A wide map 224×112: scale 0.5, centered by y.
    let view = minimap_view(224, 112);
    assert_eq!(view.scale, 0.5);
    assert_eq!((view.width, view.height), (112, 56));
    assert_eq!(view.offset_y, 28);

    // A tall map 56×112: centered by x.
    let view = minimap_view(56, 112);
    assert_eq!((view.width, view.height), (56, 112));
    assert_eq!(view.offset_x, 28);
}

#[test]
fn minimap_click_maps_to_grid_and_accounts_for_2x() {
    let viewport = Viewport::new(640, 480);

    assert_eq!(
        minimap_grid_at(15, 356, 112, 112, &viewport, false),
        Some((0, 0))
    );
    assert_eq!(
        minimap_grid_at(15 + 56, 356 + 56, 112, 112, &viewport, false),
        Some((56, 56))
    );
    assert_eq!(
        minimap_grid_at(15 + 111, 356 + 111, 112, 112, &viewport, false),
        Some((111, 111))
    );

    // 2x: camera center (56,56) → offset 28, the top-left corner shows
    // cell 28, the center — 56.
    assert_eq!(
        minimap_grid_at(15, 356, 112, 112, &viewport, true),
        Some((28, 28))
    );
    assert_eq!(
        minimap_grid_at(15 + 56, 356 + 56, 112, 112, &viewport, true),
        Some((56, 56))
    );

    assert_eq!(minimap_grid_at(14, 356, 112, 112, &viewport, false), None);
    assert_eq!(minimap_grid_at(15, 468, 112, 112, &viewport, false), None);
}

#[test]
fn darkening_table_halves_brightness_like_cpp() {
    let mut palette = [[0u8, 0, 0, 255]; 256];
    palette[2] = [100, 100, 100, 255];
    palette[128] = [48, 48, 48, 255];
    let table = darkening_table(&palette);

    // `max(7,100/4)*31/63 = 12`; the nearest palette color — index 128.
    assert_eq!(table[2], 128);
    assert_eq!(table[0], 0, "чёрный остаётся чёрным");
}

#[test]
fn minimap_unit_dots_use_team_colors_and_tnt_filter() {
    use max_core::enums::{unit_flags, UnitOrder, UnitType};
    use max_core::geo::Point;

    let mut state = max_core::universe::GameUniverse::default();
    let id = state
        .spawn_unit(
            UnitType::Tank,
            max_core::TeamId(0),
            Point::new(10, 10),
            unit_flags::SELECTABLE,
        )
        .expect("юнит");
    state.unit_mut(id).expect("юнит").orders = UnitOrder::Move;

    let mut layer = vec![0u8; 112 * 112];
    paint_minimap_units(
        &mut layer,
        StateRef::from(&state),
        112,
        112,
        false,
        false,
        false,
        None,
    );

    // 2×2 unit dot, RED color (index 1).
    assert_eq!(layer[10 * 112 + 10], 1);
    assert_eq!(layer[11 * 112 + 11], 1);

    // TNT filter: the unit has no firing sprite — the dot disappears.
    {
        let unit = state.unit_mut(id).expect("юнит");
        unit.flags &= !(unit_flags::HAS_FIRING_SPRITE | unit_flags::FIRES_MISSILES);
    }
    let mut filtered = vec![0u8; 112 * 112];
    paint_minimap_units(
        &mut filtered,
        StateRef::from(&state),
        112,
        112,
        true,
        false,
        false,
        None,
    );
    assert_eq!(filtered[10 * 112 + 10], 0);

    // In 2x a non-building is drawn as one cell.
    let mut zoomed = vec![0u8; 112 * 112];
    paint_minimap_units(
        &mut zoomed,
        StateRef::from(&state),
        112,
        112,
        false,
        true,
        false,
        None,
    );
    assert_eq!(zoomed[10 * 112 + 10], 1);
    assert_eq!(zoomed[11 * 112 + 11], 0);
}

#[test]
fn unit_dot_colors_follow_the_cpp_hash_teams() {
    assert_eq!(minimap_unit_color(max_core::TeamId(0)), 0x01);
    assert_eq!(minimap_unit_color(max_core::TeamId(1)), 0x02);
    assert_eq!(minimap_unit_color(max_core::TeamId(2)), 0x03);
    assert_eq!(minimap_unit_color(max_core::TeamId(3)), 0xFF);
    assert_eq!(minimap_unit_color(max_core::TeamId(4)), 0x04);
}

#[test]
fn panels_load_and_paint_every_region() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let max_res = root.join("max-original/MAX.RES");
    let color_pal = root.join("max-original/COLOR.PAL");
    let patches = root.join("tests/fixtures/assets/PATCHES.RES");

    assert!(max_res.is_file() && color_pal.is_file() && patches.is_file(), concat!("required test fixture is missing: ", stringify!(!max_res.is_file() || !color_pal.is_file() || !patches.is_file())));

    let config = crate::AppConfig {
        max_res: max_res.clone(),
        color_pal,
        patches_res: patches.clone(),
        ..Default::default()
    };
    let mut app = crate::App::new(config);
    app.init_assets().expect("ассеты игры загружаются");
    assert!(
        app.assets().is_some_and(|assets| assets.panels.is_some()),
            "Panels::load из MAX.RES"
    );

    let panels = app
        .assets()
        .and_then(|assets| assets.panels.as_ref())
        .expect("панели");
    let mut frame = RgbaBuffer::filled(640, 480, Rgba::rgb(1, 2, 3));

    let painted = panels.draw(&mut frame, &app);
    assert!(painted > 0);

    for rect in [
        layout::FILES_BUTTON,
        layout::PREFS_BUTTON,
        layout::END_TURN_BUTTON,
        layout::TURN_COUNTER,
        layout::TURN_TIMER,
        layout::CORNER_FLIC,
        layout::ZOOM_SLIDER,
        layout::COORDINATES,
        layout::UNIT_DESCRIPTION,
        layout::MINIMAP,
    ] {
        assert!(
            (0..rect.height).any(|y| (0..rect.width).any(|x| {
                frame.get(rect.ulx + x, rect.uly + y) != Some(Rgba::rgb(1, 2, 3))
            })),
                "зона {rect:?} не закрашена"
        );
    }
}

// -----------------------------------------------------------------------
// Statistics rows (`GameManager_UpdateInfoDisplay`)
// -----------------------------------------------------------------------

/// A match with real team attributes (`TeamUnits::with_clan(Random)`).
fn stats_state() -> max_core::universe::GameUniverse<max_core::universe::LocalDispatcher> {
    use max_core::team::TeamUnits;

    let mut state = max_core::universe::GameUniverse::default();
    *state.team_units_mut(TeamId::RED).expect("RED economy") =
        TeamUnits::with_clan(max_core::enums::TeamClan::Random);
    state.set_player_team(TeamId::RED);

    state
}
/// A unit in the match: `spawn_unit` + `DeployUnit` (a new unit has `storage = 0`).
fn deployed(
    state: &mut max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
    unit_type: UnitType,
) -> UnitInfo {
    let id = state
        .spawn_unit(unit_type, TeamId::RED, max_core::geo::Point::new(20, 20), 0)
        .expect("юнит создан");

    state.unit_mut(id).expect("юнит в состоянии").storage = 0;

    state.snapshot().unit(id).expect("юнит").clone()
}

fn labels(rows: &[Option<StatRow>; 4]) -> [Option<&'static str>; 4] {
    rows.map(|row| row.map(|row| row.label.caption()))
}

/// A mine (own stationary `RAW` storage): `Hits/Cargo/Total`, without `Speed`.
#[test]
fn mining_station_panel_shows_hits_cargo_total_without_speed() {
    let mut state = stats_state();
    let unit = deployed(&mut state, UnitType::MiningStation);
    let rows = rows_for(StateRef::from(&state), &unit, TeamId::RED);

    assert_eq!(
        labels(&rows),
        [Some("Hits"), Some("Cargo"), Some("Total"), None],
            "C++: Hits, Cargo (ATTRIB_STORAGE 25), запас комплекса (d28b); Speed 0 — пропущен"
    );
    assert!(
        rows.iter()
            .flatten()
            .all(|row| row.label != StatLabel::Speed),
            "у шахты не должно быть Speed: {rows:?}"
    );
    // `MiningStation`: 56 hits, storage 25, cargo type RAW.
    assert_eq!(rows[0].expect("Hits").text(), "56/56");
    assert_eq!(rows[1].expect("Cargo").text(), "0/25");
    assert_eq!(rows[2].expect("Total").text(), "0/25");
    assert_eq!(rows[0].expect("Hits").color(), GREEN);
    assert_eq!(rows[1].expect("Cargo").color(), RED);
    assert_eq!(rows[2].expect("Total").color(), RED);
    // Row 4 (Shots) is empty: `min(ROUNDS, AMMO) == 0`.
    assert_eq!(rows[3], None);
}

/// Regression for the manual defect «у шахты TOTAL индицируется иконками
/// рабочих»: the complex stock row of a mine takes the **cargo** family.
///
/// Oracle: `GameManager_UpdateInfoDisplay` draws `_(d28b)` with
/// `ReportStats_CargoIcons[cargo_type * 2]` (`game_manager.cpp:7772-7775`,
/// `reportstats.cpp:56-59`) — `SI_RAW`/`EI_RAW` for a mine, `SI_FUEL`/
/// `EI_FUEL` for a fuel storage. `SI_WORK` belongs to the worker rows of
/// `DrawInfoDisplayType1` only (`game_manager.cpp:7528-7576`).
#[test]
fn complex_total_row_takes_the_cargo_icon_family() {
    let mut state = stats_state();
    let mine = deployed(&mut state, UnitType::MiningStation);
    let rows = rows_for(StateRef::from(&state), &mine, TeamId::RED);

    assert_eq!(
        rows[2].expect("Total").icon,
        StatIcon::Raw,
            "_(d28b) шахты — SI_RAW (ReportStats_CargoIcons[RAW * 2])"
    );
    assert_eq!(
        rows[1].expect("Cargo").icon,
        StatIcon::Raw,
            "_(1274) шахты — SI_RAW"
    );

    let mut state = stats_state();
    let dump = deployed(&mut state, UnitType::FuelStorage);
    let rows = rows_for(StateRef::from(&state), &dump, TeamId::RED);

    assert_eq!(
        rows[2].expect("Total").icon,
        StatIcon::Fuel,
            "_(d28b) топливного склада — SI_FUEL"
    );
}

/// Regression for the «заполнение наоборот» half of the same defect: a row with
/// nothing left is drawn with the **empty** (`EI_*`, contour) pips, not with the
/// full ones.
///
/// Oracle: `GameManager_DrawInfoDisplayRowIcons` (`game_manager.cpp:7304-7310`)
/// picks `icon + 1` (the empty image) when `current_value == 0`, and switches
/// to it after the filled pips (`:7360-7365`).
#[test]
fn a_spent_row_draws_the_empty_pips_and_a_full_one_the_filled_pips() {
    let mut images = std::collections::BTreeMap::new();
    images.insert("SI_RAW", solid_image(0x01));
    images.insert("EI_RAW", solid_image(0x02));
    let icons = StatIcons { images };
    let palette = test_palette();
    let row = layout::STAT_ROW_2;
    let strip = PixelRect::new(row.ulx + 75, row.uly, row.width - 75, row.height);

    let count = |frame: &RgbaBuffer, color: Rgba| -> usize {
        let mut count = 0;
        for y in strip.uly..strip.lry() {
            for x in strip.ulx..strip.lrx() {
                if frame.get(x, y) == Some(color) {
                    count += 1;
                }
            }
        }
        count
    };
    let draw = |current: i32| -> RgbaBuffer {
        let mut frame = RgbaBuffer::filled(640, 480, Rgba::from_rgb([0, 0, 0]));
        let stat = StatRow {
            label: StatLabel::Cargo,
            icon: StatIcon::Raw,
            current,
            base: 10,
            factor: 1,
        };

        draw_stat_row_icons(&mut frame, row, &stat, &palette, &icons);
        frame
    };

    let empty = palette.rgba(0x02);
    let full = palette.rgba(0x01);

    // Nothing stored: every pip is the contour one.
    let spent = draw(0);
    assert_eq!(count(&spent, full), 0, "заполненных иконок быть не должно");
    assert!(count(&spent, empty) > 0, "пустая строка — контурные иконки");

    // Full: the opposite.
    let filled = draw(10);
    assert_eq!(count(&filled, empty), 0, "пустых иконок быть не должно");
    assert!(
        count(&filled, full) > 0,
            "полная строка — заполненные иконки"
    );

    // Half: both families are present.
    let half = draw(5);
    assert!(count(&half, full) > 0, "половина — заполненные");
    assert!(count(&half, empty) > 0, "половина — контурные");
}

/// A solid `SimpleImage` of the given palette index (row pip stub).
fn solid_image(color: u8) -> max_assets::SimpleImage {
    let mut bytes = vec![4u8, 0, 4, 0, 0, 0, 0, 0];
    // `pixels[0]` is the transparent index; the rest is the pip colour.
    bytes.push(0);
    bytes.extend(std::iter::repeat(color).take(15));

    max_assets::SimpleImage::decode(&bytes).expect("simple image")
}

/// A tank (mobile ground with an attack): `Hits/Ammo/Speed/Shots`.
#[test]
fn tank_panel_keeps_speed_ammo_and_shots() {
    let mut state = stats_state();
    let unit = deployed(&mut state, UnitType::Tank);
    let rows = rows_for(StateRef::from(&state), &unit, TeamId::RED);

    assert_eq!(
        labels(&rows),
        [Some("Hits"), Some("Ammo"), Some("Speed"), Some("Shots")]
    );
    assert_eq!(rows[0].expect("Hits").text(), "24/24");
    assert_eq!(rows[1].expect("Ammo").text(), "14/14");
    assert_eq!(rows[2].expect("Speed").text(), "6/6");
    assert_eq!(rows[3].expect("Shots").text(), "2/2");
    assert!(rows.iter().flatten().all(|row| row.color() == GREEN));
}

/// A turret (stationary, with an attack): `Hits/Ammo/Shots`, no `Speed`.
#[test]
fn turret_panel_shows_ammo_and_shots_without_speed() {
    let mut state = stats_state();
    let unit = deployed(&mut state, UnitType::GunTurret);
    let rows = rows_for(StateRef::from(&state), &unit, TeamId::RED);

    assert_eq!(
        labels(&rows),
        [Some("Hits"), Some("Ammo"), None, Some("Shots")]
    );
    assert!(rows
        .iter()
        .flatten()
        .all(|row| row.label != StatLabel::Speed));
    assert_eq!(rows[3].expect("Shots").text(), "2/2");
}

/// A scout: `Hits/Speed`, empty `Ammo` and `Shots` (ATTACK = 0, ROUNDS = 0).
#[test]
fn surveyor_panel_skips_empty_attribute_rows() {
    let mut state = stats_state();
    let unit = deployed(&mut state, UnitType::Surveyor);
    let rows = rows_for(StateRef::from(&state), &unit, TeamId::RED);

    assert_eq!(
        labels(&rows),
        [Some("Hits"), None, Some("Speed"), None],
            "C++: ATTRIB_STORAGE 0 → Cargo нет, ATTACK 0 → Ammo нет"
    );
    assert_eq!(rows[0].expect("Hits").text(), "16/16");
    assert_eq!(rows[2].expect("Speed").text(), "7/7");
}

/// A factory: `Hits/USAGE(RAW)/Total` of workers (`DrawInfoDisplayType1`),
/// without `Speed`.
///
/// Port enhancement: the `USAGE` row of a producing factory is the **actual raw
/// per turn** (`Cargo_GetRawConsumptionRate`, `SI_RAW`), not the worker need;
/// while the factory is off the row is empty.
#[test]
fn factory_panel_shows_raw_usage_only_while_producing() {
    let mut state = stats_state();
    let unit = deployed(&mut state, UnitType::TrainingHall);
    let rows = rows_for(StateRef::from(&state), &unit, TeamId::RED);

    // Off (no build order): no USAGE row; row 3 (`Total` of workers) still
    // reports the complex's worker balance `0/1`.
    assert_eq!(
        labels(&rows),
        [Some("Hits"), None, Some("Total"), None],
            "выключенный завод: USAGE не отображается"
    );
    assert!(rows
        .iter()
        .flatten()
        .all(|row| row.label != StatLabel::Speed));

    // Producing: USAGE = raw per turn at `build_rate = 1`
    // (`Cargo_GetRawConsumptionRate(TRAINHAL, 1) = 1`).
    let mut producing = unit.clone();
    producing.orders = UnitOrder::Build;
    producing.state = UnitOrderState::BuildInProgress;
    producing.build_time = 4;
    producing.build_rate = 1;
    producing.build_list = vec![UnitType::Infantry];
    let rows = rows_for(StateRef::from(&state), &producing, TeamId::RED);

    let usage = rows[1].expect("Usage raw");
    assert_eq!(usage.label, StatLabel::Usage);
    assert_eq!(usage.icon, StatIcon::Raw, "расход RAW — SI_RAW");
    assert_eq!(usage.text(), "1/1", "1 RAW в ход при build_rate = 1");
    assert_eq!(usage.color(), GREEN);

    // 2× rate: `Cargo_GetRawConsumptionRate(1 * 2 → 4)` — the actual rate.
    let mut fast = producing.clone();
    fast.build_rate = 2;
    let rows = rows_for(StateRef::from(&state), &fast, TeamId::RED);

    assert_eq!(rows[1].expect("Usage raw").text(), "4/4");

    // Paused by a shortage: the factory spends nothing — no USAGE row.
    let mut paused = producing.clone();
    paused.build_paused = true;
    let rows = rows_for(StateRef::from(&state), &paused, TeamId::RED);

    assert_eq!(rows[1], None, "пауза: фактического расхода нет");
}

/// A power plant: `Hits/Power/Total` (`DrawInfoDisplayType2`).
#[test]
fn power_station_panel_shows_power_and_total() {
    let mut state = stats_state();
    let unit = deployed(&mut state, UnitType::PowerStation);
    let rows = rows_for(StateRef::from(&state), &unit, TeamId::RED);

    assert_eq!(
        labels(&rows),
        [Some("Hits"), Some("Power"), Some("Total"), None],
            "POWERSTN: `Cargo_GetPowerConsumptionRate = -6`, приказ POWER_OFF"
    );
    assert_eq!(rows[1].expect("Power").text(), "0/6");
    assert_eq!(rows[2].expect("Total").text(), "0/6");
    assert_eq!(rows[3], None, "Usage: база (потребление) 0 — строка пустая");
}

/// A habitat: `Hits/Teams/Total` of workers (`life = -3`).
#[test]
fn habitat_panel_shows_teams_and_total() {
    let mut state = stats_state();
    let unit = deployed(&mut state, UnitType::Habitat);
    let rows = rows_for(StateRef::from(&state), &unit, TeamId::RED);

    assert_eq!(
        labels(&rows),
        [Some("Hits"), Some("Teams"), Some("Total"), None]
    );
    assert_eq!(rows[1].expect("Teams").text(), "3/3");
    assert_eq!(rows[2].expect("Total").text(), "3/3");
    assert_eq!(rows[1].expect("Teams").color(), GREEN);
}

/// A foreign unit: `Ammo`/`Cargo` are not drawn (`factor == 0` in C++).
#[test]
fn enemy_unit_hides_ammo_and_cargo_rows() {
    let mut state = stats_state();
    let unit = deployed(&mut state, UnitType::Tank);
    let mut enemy = unit;
    enemy.team = TeamId::GREEN;

    let rows = rows_for(StateRef::from(&state), &enemy, TeamId::RED);

    assert_eq!(
        labels(&rows),
        [Some("Hits"), None, Some("Speed"), Some("Shots")]
    );
}

/// The three `MessageBoxStyle` background tables (`MessageManager_
/// MessageBox_BgColorArray`): info darkens, warning tints red, notice is neutral
/// on a normal tileset and red on snow.
#[test]
fn message_box_styles_follow_the_cpp_colour_tables() {
    let mut colors = [[0u8; 3]; 256];
    let entries = [
        [0, 0, 0],
        [255, 0, 0],
        [100, 100, 100],
        [50, 50, 50],
        [25, 25, 25],
        [255, 255, 255],
        [200, 0, 0],
        [64, 0, 0],
        [81, 81, 81],
        [40, 40, 40],
    ];

    colors[..entries.len()].copy_from_slice(&entries);

    let palette = UiPalette::from_colors(colors);
    let gray = Rgba::rgb(100, 100, 100);

    // `max(7, c) * 31 / 63` → 49 → the nearest palette entry is (50, 50, 50).
    assert_eq!(
        palette.message_box_color(MessageBoxStyle::Info, false, gray),
        Rgba::rgb(50, 50, 50)
    );

    // `(max(c, 63/4) * 63) / 31` → (203, 0, 0) → (200, 0, 0).
    let warning = palette.message_box_color(MessageBoxStyle::Warning, false, gray);
    assert!(
        warning.r > warning.g && warning.g == warning.b,
            "{warning:?}"
    );

    // `GenerateIntensityTable2`: neutral grey on green/desert/crater.
    let notice = palette.message_box_color(MessageBoxStyle::Notice, false, gray);
    assert!(notice.r == notice.g && notice.g == notice.b, "{notice:?}");

    // …and red on the snow tilesets.
    let snow = palette.message_box_color(MessageBoxStyle::Notice, true, gray);
    assert!(snow.r > snow.g && snow.g == snow.b, "{snow:?}");
}

/// The message window paints its whole rectangle with the style table and the
/// text in white with a black outline (`MessageManager_DrawMessageBox`).
#[test]
fn warning_message_box_paints_a_red_tint_behind_white_text() {
    let mut colors = [[0u8; 3]; 256];
    let entries = [
        [0, 0, 0],
        [255, 255, 255],
        [100, 100, 100],
        [49, 49, 49],
        [203, 0, 0],
        [203, 0, 0],
        [81, 81, 81],
        [40, 40, 40],
    ];

    colors[..entries.len()].copy_from_slice(&entries);
    // `MessageManager_DrawMessageBoxText` draws in palette index `0xFF`.
    colors[0xFF] = [255, 255, 255];

    let palette = UiPalette::from_colors(colors);
    let background = Rgba::rgb(100, 100, 100);
    let mut frame = RgbaBuffer::filled(160, 60, background);

    draw_status_message(
        &mut frame,
        PixelRect::new(0, 0, 160, 60),
        &palette,
            "HX",
        MessageBoxStyle::Warning,
        false,
    );

    // The box starts at `(area.ulx, area.uly + 10)`; its corner is the tinted
    // background, not the untouched map pixel.
    let corner = frame.get(0, 10).expect("пиксель окна");
    assert_eq!(
        corner,
        palette.message_box_color(MessageBoxStyle::Warning, false, background)
    );
    assert!(corner.r > corner.g + corner.b, "красный фон: {corner:?}");

    // `Text_Blit` with `GNW_TEXT_OUTLINE`: the white fill over the black copy.
    let white = frame
        .pixels()
        .chunks_exact(4)
        .filter(|pixel| pixel[0] == 255 && pixel[1] == 255 && pixel[2] == 255)
        .count();

    assert!(white > 0, "белый текст поверх красного фона");
}

/// Colors — the `GameManager_DrawInfoDisplayRow` thresholds (1/4 and 1/2 of the base).
#[test]
fn stat_row_colors_follow_the_cpp_thresholds() {
    let row = |current, base, factor| StatRow {
        label: StatLabel::Hits,
        icon: StatIcon::Hits,
        current,
        base,
        factor,
    };

    assert_eq!(row(1, 4, 1).color(), RED);
    assert_eq!(row(2, 4, 1).color(), YELLOW);
    assert_eq!(row(3, 4, 1).color(), GREEN);
    assert_eq!(row(0, 1, 1).color(), RED);
    assert_eq!(row(1, 1, 1).color(), GREEN);
    assert_eq!(row(0, 0, 1).color(), TEXT);
    // `%i/%i` and `std::min(current, base)` as in C++.
    assert_eq!(
        make_row(StatLabel::Hits, 99, 56, 4).expect("строка").text(),
            "56/56"
    );
    assert_eq!(make_row(StatLabel::Hits, 0, 0, 4), None);
    assert_eq!(make_row(StatLabel::Cargo, 0, 25, 0), None);
}

/// Regression for the manual defect "mine Cargo 25 Raw, complex Total 50":
/// the `Total` row must sum `Cargo_GetCargoCapacity` of **every** complex
/// member (`complex.cpp:136-147`) — the mine's 25-raw buffer plus the
/// storage's 50, not the storage alone.
#[test]
fn complex_total_includes_the_mine_capacity() {
    let mut state = stats_state();
    let mine = state
        .spawn_unit(
            UnitType::MiningStation,
            TeamId::RED,
            max_core::geo::Point::new(20, 20),
            0,
        )
        .expect("mine");
    let dump = state
        .spawn_unit(
            UnitType::MaterialStorage,
            TeamId::RED,
            max_core::geo::Point::new(22, 20),
            0,
        )
        .expect("storage");
    state.unit_mut(mine).expect("mine").storage = 0;
    state.unit_mut(dump).expect("storage").storage = 0;

    let snapshot = state.snapshot();
    assert_eq!(
        snapshot.unit(mine).expect("mine").complex,
        snapshot.unit(dump).expect("storage").complex,
            "the mine and the storage must merge into one complex"
    );

    let unit = snapshot.unit(mine).expect("mine").clone();
    let rows = rows_for(snapshot, &unit, TeamId::RED);

    assert_eq!(rows[1].expect("Cargo").text(), "0/25", "mine Cargo");
    assert_eq!(
        rows[2].expect("Total").text(),
            "0/75",
            "MININGST 25 + ADUMP 50 in the complex capacity"
    );
}

/// The oracle of the manual defect: `ComplexId` is unique only within a team,
/// while C++ compares `Complex*`. Both starting bases own complex 2, so a mine
/// with Cargo `12/25` showed `Total 12/50` — its own 25 plus the **enemy**
/// mine's 25 of the same numeric complex id.
#[test]
fn complex_total_ignores_a_foreign_complex_with_the_same_id() {
    use max_core::team::TeamUnits;

    let mut state = stats_state();
    *state.team_units_mut(TeamId::GREEN).expect("GREEN economy") =
        TeamUnits::with_clan(max_core::enums::TeamClan::Random);

    let own_mine = state
        .spawn_unit(
            UnitType::MiningStation,
            TeamId::RED,
            max_core::geo::Point::new(20, 20),
            0,
        )
        .expect("own mine");
    let enemy_mine = state
        .spawn_unit(
            UnitType::MiningStation,
            TeamId::GREEN,
            max_core::geo::Point::new(60, 60),
            0,
        )
        .expect("enemy mine");
    state.unit_mut(own_mine).expect("mine").storage = 12;
    state.unit_mut(enemy_mine).expect("mine").storage = 0;

    let snapshot = state.snapshot();
    assert_eq!(
        snapshot.unit(own_mine).expect("mine").complex,
        snapshot.unit(enemy_mine).expect("mine").complex,
            "the fixture reproduces the numeric id collision"
    );

    let unit = snapshot.unit(own_mine).expect("mine").clone();
    let rows = rows_for(snapshot, &unit, TeamId::RED);

    assert_eq!(rows[1].expect("Cargo").text(), "12/25");
    assert_eq!(
        rows[2].expect("Total").text(),
            "12/25",
            "the enemy complex with the same id must not count"
    );
}

/// The same team filter in the power/workers `Total` rows: two teams'
/// `POWGEN`s share a numeric complex id, only the own one is counted.
#[test]
fn power_total_ignores_a_foreign_complex_with_the_same_id() {
    use max_core::team::TeamUnits;

    let mut state = stats_state();
    *state.team_units_mut(TeamId::GREEN).expect("GREEN economy") =
        TeamUnits::with_clan(max_core::enums::TeamClan::Random);

    let own = state
        .spawn_unit(
            UnitType::PowerGenerator,
            TeamId::RED,
            max_core::geo::Point::new(20, 20),
            0,
        )
        .expect("own generator");
    let enemy = state
        .spawn_unit(
            UnitType::PowerGenerator,
            TeamId::GREEN,
            max_core::geo::Point::new(60, 60),
            0,
        )
        .expect("enemy generator");

    let snapshot = state.snapshot();
    assert_eq!(
        snapshot.unit(own).expect("own").complex,
        snapshot.unit(enemy).expect("enemy").complex,
            "the fixture reproduces the numeric id collision"
    );

    let unit = snapshot.unit(own).expect("own").clone();
    let rows = rows_for(snapshot, &unit, TeamId::RED);
    let total = rows[2].expect("Total");

    assert_eq!(
        total.text(),
            "0/1",
            "only the own generator gives its 1 power"
    );
}

/// The `Speed` row exists exactly for types with `ATTRIB_SPEED > 0` (all 93 types).
#[test]
fn speed_row_appears_only_with_a_speed_attribute() {
    use max_core::unit_values::base_unit_values;

    let state = stats_state();

    for &unit_type in UnitType::ALL {
        let base = base_unit_values(unit_type);
        let mut unit = UnitInfo::new(unit_type, TeamId::RED, max_core::UnitId(1));
        unit.base_values = base;
        unit.hits = base.hits;
        unit.speed = base.speed;
        unit.shots = base.rounds as u8;
        unit.ammo = base.ammo as u8;
        unit.storage = 0;

        let rows = rows_for(StateRef::from(&state), &unit, TeamId::RED);
        let has_speed = rows
            .iter()
            .flatten()
            .any(|row| row.label == StatLabel::Speed);

        assert_eq!(has_speed, base.speed > 0, "{unit_type:?}");
    }
}
