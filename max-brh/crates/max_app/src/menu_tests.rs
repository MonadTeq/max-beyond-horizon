use super::*;

use std::path::PathBuf;

fn repo_path(relative: &str) -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn test_assets() -> Option<(max_assets::ResArchive, MenuAssets)> {
    let max_res = repo_path("max-original/MAX.RES");

    if !max_res.is_file() {
        return None;
    }

    let archive = max_assets::ResArchive::open(&max_res).ok()?;
    let menu = MenuAssets::load(&archive)?;

    Some((archive, menu))
}

#[test]
fn main_menu_layout_matches_the_cpp_oracle() {
    // `menu.cpp::main_menu`: 6 big buttons 200×29 at x=385 with a step of 35,
    // Exit 100×30 at (435,420).
    for (index, button) in MAIN_MENU_BUTTONS.iter().take(6).enumerate() {
        assert_eq!(
            button.rect,
            PixelRect::new(385, 175 + index as i32 * 35, 200, 29)
        );
        assert!(button.big);
    }

    let exit = MAIN_MENU_BUTTONS[6];
    assert_eq!(exit.rect, PixelRect::new(435, 420, 100, 30));
    assert!(!exit.big);
    assert_eq!(exit.action, MainMenuAction::Quit);

    assert_eq!(MAIN_MENU_BUTTONS[0].label, "New Game");
    assert_eq!(MAIN_MENU_BUTTONS[2].label, "Multiplayer Game");
    assert_eq!(MAIN_MENU_BUTTONS[6].label, "Exit");

    // Bottom row buttons — `planetselectmenu.hpp` / `networkmenu.hpp`.
    assert_eq!(PLANET_BUTTONS[0].rect, PixelRect::new(243, 438, 109, 40));
    assert_eq!(PLANET_BUTTONS[3].rect, PixelRect::new(514, 438, 109, 40));
    assert_eq!(JOIN_LIST, PixelRect::new(359, 28, 209, 160));
    assert_eq!(JOIN_ROW_HEIGHT, 20);
}

#[test]
fn clan_logos_are_drawn_at_their_anchor() {
    // `WindowManager_DecodeSimpleImage`: `ulx -= image->ulx`, `uly -= image->uly`.
    let Some((_archive, menu)) = test_assets() else {
        return;
    };

    let logo = menu.clan_logos[1].as_ref().expect("CLN1LOGO");
    // CLN1LOGO: 74×57, ulx=37, uly=28 — the anchor inside the picture.
    assert_eq!((logo.width, logo.height), (74, 57));
    assert_eq!((logo.ulx, logo.uly), (37, 28));

    let mut frame = RgbaBuffer::filled(640, 480, Rgba::rgb(0, 0, 0));
    blit_simple_anchored(&mut frame, &menu.palette, logo, 41, 40, true);

    // Rectangle of the drawn area: from (41-37, 40-28) = (4, 12) and no wider
    // than the picture — otherwise (as before the fix) the logo went beyond the button.
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for y in 0..480 {
        for x in 0..640 {
            if frame.get(x, y) != Some(Rgba::rgb(0, 0, 0)) {
                xs.push(x);
                ys.push(y);
            }
        }
    }

    assert!(!xs.is_empty(), "логотип нарисован");
    assert!(xs.iter().min().copied().unwrap() >= 4);
    assert!(ys.iter().min().copied().unwrap() >= 12);
    assert!(
        xs.iter().max().copied().unwrap() <= 4 + 74 - 1,
            "логотип не должен выходить за 74 px от левого края картинки"
    );
    assert!(ys.iter().max().copied().unwrap() <= 12 + 57 - 1);
}

#[test]
fn quest_caption_is_centered_like_cpp() {
    // `Button::SetCaption` + `Text_TextLine` (`button.cpp:433`, `text.cpp:778`):
    //   uly = ((40 - 16) / 2) = 12; ulx = (48 - (Text_GetWidth+1)) / 2.
    let font = GnwFontId::Font1.font();
    let text_width = font.text_width("?") + 1;
    let expected_x = 465 + (48 - text_width).max(0) / 2;
    let expected_y = 438 + (40 - font.height()).max(0) / 2;

    assert_eq!(expected_x, 484, "знак ? в кнопке 48×40");
    assert_eq!(expected_y, 450);

    let mut frame = RgbaBuffer::filled(640, 480, Rgba::rgb(0, 0, 0));
    draw_text_gnw_centered(
        &mut frame,
        PixelRect::new(465, 438, 48, 40),
        font,
            "?",
        Rgba::WHITE,
    );

    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for y in 438..478 {
        for x in 465..513 {
            if frame.get(x, y) == Some(Rgba::WHITE) {
                xs.push(x);
                ys.push(y);
            }
        }
    }

    assert_eq!(xs.iter().min().copied(), Some(expected_x));
    // The '?' "ink" starts from the second row of the cell (ink bbox y0 = 1).
    assert_eq!(ys.iter().min().copied(), Some(expected_y + 1));
    // Horizontally the sign is indeed inside the button, not at the edge.
    assert!(xs.iter().max().copied().unwrap() < 513);
    assert!(ys.iter().max().copied().unwrap() < 478);
}

#[test]
fn menu_assets_load_all_button_images_from_max_res() {
    let Some((_archive, menu)) = test_assets() else {
        return;
    };

    assert_eq!(menu.blank.size(), (200, 29));
    assert_eq!(menu.small_blank.size(), (100, 30));

    for index in 0..6 {
        let pair = &menu.mnubtn[index];
        let expected = if index == 4 { (48, 40) } else { (109, 40) };
        assert_eq!(pair.size(), expected, "MNUBTN{}", index + 1);
    }

    assert_eq!(
        (menu.mainpic.width, menu.mainpic.height),
        (640, 480),
            "MAINPIC — фон главного меню"
    );
    assert_eq!(
        menu.planet_background().width,
        640,
            "PLANETSE или fallback 640×480"
    );
    assert_eq!(menu.join_background().width, 640);
    assert!(menu.portrait.is_some(), "P_* портрет 300×240");
    assert!(menu.btnhide.is_some(), "BTNHIDE 38×21");
    assert!(menu.portrait_frame_left.is_some());
    assert!(menu.portrait_frame_right.is_some());
}

#[test]
fn main_menu_draws_mainpic_title_and_gold_captions() {
    let Some((_archive, menu)) = test_assets() else {
        return;
    };
    let mut frame = RgbaBuffer::new(640, 480);

    draw_main_menu(&mut frame, &menu, "", Some((390, 180)));

    // Background — MAINPIC (0,0 in its palette).
    assert_eq!(
        frame.get(0, 0),
        menu.mainpic.rgb(0, 0).map(Rgba::from_rgb),
            "фон главного меню — MAINPIC"
    );

    let gold = menu.palette.rgba(165);
    let mut gold_pixels = 0;

    for y in 160..460 {
        for x in 380..600 {
            if frame.get(x, y) == Some(gold) {
                gold_pixels += 1;
            }
        }
    }

    assert!(
        gold_pixels > 50,
            "подписи кнопок — Fonts_GoldColor: {gold_pixels}"
    );

    // Mouse hover does not press the button: the frame with the cursor is bitwise equal to
    // the frame without the cursor (in the original `*D` — only while the button is pressed).
    let mut idle = RgbaBuffer::new(640, 480);
    draw_main_menu(&mut idle, &menu, "", None);
    assert_eq!(
        frame.pixels(),
        idle.pixels(),
            "наведение не должно рисовать нажатую кнопку *D"
    );
}

#[test]
fn create_form_draws_planetse_grid_and_mnubtn_buttons() {
    let Some((_archive, menu)) = test_assets() else {
        return;
    };
    let mut frame = RgbaBuffer::new(640, 480);
    let form = CreateForm {
        world: 12,
        ..Default::default()
    };

    draw_create_form(&mut frame, &menu, None, &form, "status", None);

    // Background — PLANETSE (if it exists in MAX.RES).
    if let Some(ref planetse) = menu.planetse {
        assert_eq!(
            frame.get(600, 300),
            planetse.rgb(600, 300).map(Rgba::from_rgb),
                "фон формы — PLANETSE"
        );
    }

    // Done button — MNUBTN6U at (514,438). The first pixel of the picture —
    // transparent (C++ `Image::Blend`), so we take the frame pixel (2,2).
    let pair = menu.mnubtn_pair(6).expect("MNUBTN6");
    let source = pair
        .pixel(false, 2, 2)
        .map(|index| menu.palette.rgba(index));

    if let Some(source) = source {
        assert_eq!(frame.get(514 + 2, 438 + 2), Some(source));
    }

    // The selected planet is outlined in red.
    let cell = planet_cell(12);
    let red = Rgba::rgb(252, 0, 0);
    assert_eq!(frame.get(cell.ulx, cell.uly), Some(red));
}

#[test]
fn join_list_draws_multgame_frame_rows_and_buttons() {
    let Some((_archive, menu)) = test_assets() else {
        return;
    };
    let games = vec![
        max_net::GameInfo {
            id: "g1".to_string(),
            name: "Alpha".to_string(),
            world: 0,
            status: max_net::GameStatus::Running,
            port: 31554,
            players: 1,
            min_players: 2,
            password_protected: false,
        },
        max_net::GameInfo {
            id: "g2".to_string(),
            name: "Beta".to_string(),
            world: 0,
            status: max_net::GameStatus::Running,
            port: 31554,
            players: 2,
            min_players: 2,
            password_protected: true,
        },
    ];
    let mut frame = RgbaBuffer::new(640, 480);

    draw_join_list(&mut frame, &menu, &games, 1, "2 game(s)", None);

    if let Some(ref multgame) = menu.multgame {
        assert_eq!(
            frame.get(10, 10),
            multgame.rgb(10, 10).map(Rgba::from_rgb),
                "фон списка — MULTGAME"
        );
    }

    // Rows — in the green GNW font.
    let green = menu.palette.rgba(GREEN);
    let mut green_pixels = 0;
    for y in JOIN_LIST.uly..JOIN_LIST.uly + JOIN_LIST.height {
        for x in JOIN_LIST.ulx..JOIN_LIST.ulx + JOIN_LIST.width {
            if frame.get(x, y) == Some(green) {
                green_pixels += 1;
            }
        }
    }
    assert!(
        green_pixels > 100,
            "строки/разделители списка: {green_pixels}"
    );

    // Ready button — MNUBTN6U (frame at (2,2), the first pixel is transparent).
    let pair = menu.mnubtn_pair(6).expect("MNUBTN6");
    if let Some(source) = pair
        .pixel(false, 2, 2)
        .map(|index| menu.palette.rgba(index))
    {
        assert_eq!(frame.get(514 + 2, 438 + 2), Some(source));
    }

    // BTNHIDE is drawn in the top-right corner.
    let hide = menu.btnhide.as_ref().expect("BTNHIDE");
    if let Some(source) = hide.pixel(5, 5).map(|index| menu.palette.rgba(index)) {
        assert_eq!(
            frame.get(JOIN_HIDE.ulx + 5, JOIN_HIDE.uly + 5),
            Some(source)
        );
    }
}

#[test]
fn hit_testing_follows_the_cpp_rectangles() {
    assert_eq!(
        main_menu_at(385 + 100, 175 + 14),
        Some(MainMenuAction::Create)
    );
    assert_eq!(
        main_menu_at(385 + 100, 245 + 14),
        Some(MainMenuAction::Join)
    );
    assert_eq!(main_menu_at(435 + 50, 420 + 15), Some(MainMenuAction::Quit));
    assert_eq!(main_menu_at(100, 100), None);

    assert_eq!(
        create_button_at(243 + 10, 438 + 10).map(|b| b.action),
        Some(MenuButtonAction::Random)
    );
    assert_eq!(
        create_button_at(514 + 10, 438 + 10).map(|b| b.action),
        Some(MenuButtonAction::Done)
    );
    assert_eq!(create_field_at(48 + 2, 352 + 2), Some(CreateField::Name));
    assert_eq!(create_field_at(426 + 2, 378 + 2), Some(CreateField::World));

    assert_eq!(
        join_button_at(243 + 10, 438 + 10),
        Some(JoinButton::Refresh)
    );
    assert_eq!(join_button_at(354 + 10, 438 + 10), Some(JoinButton::Back));
    assert_eq!(join_button_at(514 + 10, 438 + 10), Some(JoinButton::Join));
    assert_eq!(
        join_button_at(JOIN_HIDE.ulx + 2, JOIN_HIDE.uly + 2),
        Some(JoinButton::Hide)
    );

    // Rows: 8 visible, 20 px.
    assert_eq!(
        join_row_at(JOIN_LIST.ulx + 5, JOIN_LIST.uly + 5, 3, 0),
        Some(0)
    );
    assert_eq!(
        join_row_at(JOIN_LIST.ulx + 5, JOIN_LIST.uly + 25, 3, 0),
        Some(1)
    );
    assert_eq!(
        join_row_at(JOIN_LIST.ulx + 5, JOIN_LIST.uly + 200, 3, 0),
        None
    );
    // Scrolling: with the 9th row selected, 2..=9 are visible, a click on row 0
    // selects the 2nd, on the 7th — the 9th.
    assert_eq!(
        join_row_at(JOIN_LIST.ulx + 5, JOIN_LIST.uly + 5, 10, 9),
        Some(2)
    );
    assert_eq!(
        join_row_at(JOIN_LIST.ulx + 5, JOIN_LIST.uly + 145, 10, 9),
        Some(9)
    );
}

#[test]
fn seven_by_three_planet_grid_fits_the_planetse_slots() {
    assert_eq!(PLANET_GRID, PixelRect::new(192, 51, 388, 249));
    assert_eq!(planet_cell(0), PixelRect::new(192, 51, 48, 83));
    assert_eq!(planet_cell(7), PixelRect::new(528, 51, 48, 83));
    assert_eq!(planet_cell(23), PixelRect::new(528, 217, 48, 83));

    for index in 0..PLANET_COUNT as i32 {
        let cell = planet_cell(index);
        assert!(cell.intersect(PLANET_GRID) == Some(cell));
    }
}
