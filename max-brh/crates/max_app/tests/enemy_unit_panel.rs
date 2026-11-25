//! DEFECT 2: inspection of a **visible enemy** unit — portrait and the four
//! statistics rows.
//!
//! Oracle — `max-cpp-port/src`:
//!
//! * `GameManager_UpdateSelection` (`game_manager.cpp:6292-6298`): an enemy under
//!   the cursor is handled by `GameManager_UnitSelectOther` (`:6229-6244`) when
//!   the click is not an attack (`CURSOR_UNIT_NO_GO`);
//! * `GameManager_GetMilitaryCursor` (`:6150-6227`) decides attack vs selection:
//!   `CURSOR_ENEMY` in range with ammo, `CURSOR_FAR_TARGET` for a mobile unit out
//!   of range, otherwise `CURSOR_UNIT_NO_GO`;
//! * `GameManager_MenuUnitSelect` (`:7141`) → `GameManager_UpdateInfoDisplay`
//!   (`:7667`) draws the portrait (`MenuCreateFlic`) and the statistics rows,
//!   gated by `IsVisibleToTeam(GameManager_PlayerTeam)`;
//! * the rows themselves hide foreign data — ammo base 0, cargo factor 0
//!   (`:7688-7720`), so a foreign tank shows `Hits`, an empty `Ammo` row,
//!   `Speed` and `Shots`.
//!
//! The port used to always route an enemy click into an attack order and never
//! selected the enemy, so its panel was unreachable.
//!
//! The tests use real frames and require the game data next to the repository
//! (like `stat_panel.rs`); a missing data set fails the test.

use std::path::{Path, PathBuf};

use max_app::hud;
use max_app::panels::UiPalette;
use max_app::{handle_session_input, App, AppConfig, InputEvent, Key};
use max_core::enums::{TeamClan, UnitOrder, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::unit::UnitId;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::layout::{self, MAIN_MAP};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// 640×480 `App` with the game assets and the chrome (panel) renderer.
fn app() -> Option<App> {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");
    // `PATCHES.RES` lives either in the delivery `assets/` directory or in the
    // C++ port's shared one (`PATCHES_RES_FALLBACKS` in `assets.rs`).
    let patches = [
        repo_path("tests/fixtures/assets/PATCHES.RES"),
        repo_path("tests/fixtures/assets/PATCHES.RES"),
        repo_path("tests/fixtures/assets/PATCHES.RES"),
    ]
    .into_iter()
    .find(|path| path.is_file());

    if !max_res.is_file() || !color_pal.is_file() {
        eprintln!("enemy_unit_panel: нет данных — нет данных игры рядом с репозиторием");
        return None;
    }

    let Some(patches) = patches else {
        eprintln!("enemy_unit_panel: нет данных — нет PATCHES.RES");
        return None;
    };

    let mut app = App::new(
        AppConfig::default()
            .with_data_dir(repo_path("max-original"))
            .with_assets_dir(patches.parent().expect("каталог PATCHES.RES")),
    );

    app.init_assets().ok()?;

    let universe = app.universe_mut();
    universe.set_player_team(TeamId::RED);
    for team in [TeamId::RED, TeamId::BLUE] {
        if let Some(info) = universe.team_mut(team) {
            info.team_type = if team == TeamId::RED {
                TeamKind::Player
            } else {
                TeamKind::Computer
            };
        }
    }
    universe.team_units_table_mut()[TeamId::RED.0 as usize] =
        TeamUnits::with_clan(TeamClan::Random);
    universe.team_units_table_mut()[TeamId::BLUE.0 as usize] =
        TeamUnits::with_clan(TeamClan::Random);

    Some(app)
}

/// Screen point of a cell center in the 640×480 frame with chrome.
fn screen_point(app: &App, grid: Point) -> (i32, i32) {
    let (x, y) = hud::tile_center(&app.map_viewport(), grid);

    (x + MAIN_MAP.ulx, y + MAIN_MAP.uly)
}

/// Text pixels (`0x01/0x02/0x04/0xA2`) of a row — the same measure as
/// `stat_panel.rs`.
fn text_pixels(frame: &RgbaBuffer, palette: &UiPalette, rect: PixelRect) -> usize {
    let colors = [
        palette.rgba(0x01),
        palette.rgba(0x02),
        palette.rgba(0x04),
        palette.rgba(0xA2),
    ];
    let mut count = 0;

    for y in rect.uly..rect.uly + rect.height {
        for x in rect.ulx..rect.ulx + rect.width {
            if frame.get(x, y).is_some_and(|pixel| colors.contains(&pixel)) {
                count += 1;
            }
        }
    }

    count
}

fn row_counts(frame: &RgbaBuffer, palette: &UiPalette) -> [usize; 4] {
    [
        layout::STAT_ROW_1,
        layout::STAT_ROW_2,
        layout::STAT_ROW_3,
        layout::STAT_ROW_4,
    ]
    .map(|rect| text_pixels(frame, palette, rect))
}

fn changed_pixels(before: &RgbaBuffer, after: &RgbaBuffer, rect: PixelRect) -> usize {
    let mut count = 0;

    for y in rect.uly..rect.lry() {
        for x in rect.ulx..rect.lrx() {
            if before.get(x, y) != after.get(x, y) {
                count += 1;
            }
        }
    }

    count
}

/// Spawns a `Surveyor` (no attack: `weapon == 0`) for RED and a `Tank` for BLUE,
/// makes the enemy visible to the player and points the camera at both.
fn world(app: &mut App) -> (UnitId, UnitId) {
    let own = app
        .universe_mut()
        .spawn_unit(UnitType::Surveyor, TeamId::RED, Point::new(30, 22), 0)
        .expect("свой Surveyor");
    let enemy = app
        .universe_mut()
        .spawn_unit(UnitType::Tank, TeamId::BLUE, Point::new(34, 22), 0)
        .expect("вражеский Tank");

    app.universe_mut()
        .unit_mut(enemy)
        .expect("вражеский Tank")
        .set_visible_to_team(TeamId::RED, true);
    app.viewport_mut().center_on_grid(32, 22);

    (own, enemy)
}

/// The regression: click an own **non-attacking** unit, then a visible enemy —
/// the enemy is selected for inspection (C++ `CURSOR_UNIT_NO_GO` →
/// `GameManager_UnitSelectOther`), no attack order is issued, and the statistics
/// panel + portrait show the enemy.
#[test]
fn clicking_a_visible_enemy_selects_it_instead_of_ordering_an_attack() {
    let mut app = app().expect(concat!("test fixture is missing: ", stringify!(app())));
    let palette = UiPalette::from_assets(app.assets().expect("ассеты")).expect("палитра FRAMEPIC");

    // No selection — the empty panel row reference (window background).
    let empty = app.render_frame();
    let baseline = row_counts(&empty, &palette);

    let (own, enemy) = world(&mut app);
    let (ox, oy) = screen_point(&app, Point::new(30, 22));
    let (ex, ey) = screen_point(&app, Point::new(34, 22));

    handle_session_input(&mut app, InputEvent::MouseClick(ox, oy));
    assert_eq!(app.hud().selected, Some(own), "свой Surveyor выделен");
    assert_eq!(
        app.state_ref().unit(own).expect("Surveyor").orders,
        UnitOrder::Await,
        "Surveyor не получил приказ от выделения"
    );

    let before_click = app.state_ref().snapshot_bytes();
    handle_session_input(&mut app, InputEvent::MouseClick(ex, ey));
    assert_eq!(
        app.state_ref().snapshot_bytes(),
        before_click,
        "выделение и осмотр — только UI, снапшот/CRC не меняются"
    );

    // C++ `GameManager_GetMilitaryCursor`: the Surveyor has no weapon, so the
    // click is not an attack — the visible enemy is selected.
    assert_eq!(
        app.hud().selected,
        Some(enemy),
        "видимый враг выделяется для просмотра характеристик"
    );
    assert!(
        app.hud()
            .log
            .lines()
            .iter()
            .any(|line| line.contains("Selected") && line.contains("Tank")),
        "журнал: Selected <враг>, получено {:?}",
        app.hud().log.lines()
    );
    assert!(
        app.hud().pending.is_empty(),
        "приказа атаки нет: {:?}",
        app.hud().pending
    );
    assert_eq!(
        app.state_ref().unit(own).expect("Surveyor").orders,
        UnitOrder::Await,
        "Surveyor остался в AWAIT"
    );

    // The panel draws the foreign rows: Hits, empty Ammo (base 0 for a foreign
    // unit), Speed, Shots — `GameManager_UpdateInfoDisplay` (`:7688-7775`).
    let frame = app.render_frame();
    let enemy_rows = row_counts(&frame, &palette);

    assert!(
        enemy_rows[0] > baseline[0],
        "Hits врага нарисован: {enemy_rows:?} vs {baseline:?}"
    );
    assert_eq!(
        enemy_rows[1], baseline[1],
        "Ammo чужого юнита скрыт (база 0): {enemy_rows:?}"
    );
    assert!(
        enemy_rows[2] > baseline[2],
        "Speed врага нарисован: {enemy_rows:?}"
    );
    assert!(
        enemy_rows[3] > baseline[3],
        "Shots врага нарисован: {enemy_rows:?}"
    );

    // The corner portrait (`WINDOW_CORNER_FLIC`, `MenuCreateFlic`) is filled for
    // the enemy too.
    assert!(
        changed_pixels(&empty, &frame, layout::CORNER_FLIC) > 0,
        "портрет врага в CORNER_FLIC не нарисован"
    );

    // A right click on the chosen enemy keeps the inspection selection and gives
    // no order (`GameManager_GetWindowCursor` is `CURSOR_UNIT_NO_GO` for a
    // foreign selected unit).
    let (mx, my) = screen_point(&app, Point::new(38, 22));
    handle_session_input(&mut app, InputEvent::RightClick(mx, my));
    assert_eq!(app.hud().selected, Some(enemy), "выделение сохранено");
    assert!(app.hud().pending.is_empty(), "правый клик не даёт приказ");

    // `O` (power) belongs to an own unit only (`GameManager_InitPopupButtons`
    // is reached for the player team, `:6253-6258`).
    handle_session_input(&mut app, InputEvent::Key(Key::Char('o')));
    assert!(
        app.hud().pending.is_empty(),
        "питание врага не переключается: {:?}",
        app.hud().pending
    );
    assert_eq!(
        app.state_ref().unit(enemy).expect("Tank").orders,
        UnitOrder::Await,
        "приказ врага не изменился"
    );
}

/// An **invisible** enemy is not selectable: C++ `Access_GetEnemyUnit`
/// (`access.cpp:1731`) requires `IsVisibleToTeam`, and `UnitSelectOther`
/// re-checks it (`game_manager.cpp:6238`).
#[test]
fn clicking_an_invisible_enemy_does_not_select_or_order() {
    let mut app = app().expect(concat!("test fixture is missing: ", stringify!(app())));
    let palette = UiPalette::from_assets(app.assets().expect("ассеты")).expect("палитра FRAMEPIC");

    let (own, enemy) = world(&mut app);
    app.universe_mut()
        .unit_mut(enemy)
        .expect("Tank")
        .set_visible_to_team(TeamId::RED, false);
    app.select_unit(None);

    let empty = app.render_frame();
    let baseline = row_counts(&empty, &palette);

    let (ex, ey) = screen_point(&app, Point::new(34, 22));
    handle_session_input(&mut app, InputEvent::MouseClick(ex, ey));

    assert_eq!(app.hud().selected, None, "невидимый враг не выделяется");
    assert!(app.hud().pending.is_empty(), "и приказ не отдаётся");
    assert_eq!(
        app.state_ref().unit(own).expect("Surveyor").orders,
        UnitOrder::Await
    );

    // The panel stays empty for the invisible enemy.
    let frame = app.render_frame();
    assert_eq!(
        row_counts(&frame, &palette),
        baseline,
        "строки характеристик скрыты (DrawStats по видимости)"
    );
    assert_eq!(
        changed_pixels(&empty, &frame, layout::CORNER_FLIC),
        0,
        "портрет невидимого врага не рисуется"
    );

    // Visibility came back with the scan — the same click now selects it.
    app.universe_mut()
        .unit_mut(enemy)
        .expect("Tank")
        .set_visible_to_team(TeamId::RED, true);
    handle_session_input(&mut app, InputEvent::MouseClick(ex, ey));

    assert_eq!(app.hud().selected, Some(enemy));
}

/// A visible enemy that the selected own unit **can** attack still gets the
/// attack order (C++ `CURSOR_ENEMY`/`CURSOR_FAR_TARGET`) — the inspection path
/// must not swallow combat clicks.
#[test]
fn an_attackable_enemy_still_receives_the_attack_order() {
    let mut app = app().expect(concat!("test fixture is missing: ", stringify!(app())));

    let tank = app
        .universe_mut()
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(30, 22), 0)
        .expect("свой Tank");
    let enemy = app
        .universe_mut()
        .spawn_unit(UnitType::Tank, TeamId::BLUE, Point::new(34, 22), 0)
        .expect("вражеский Tank");
    app.universe_mut()
        .unit_mut(enemy)
        .expect("Tank")
        .set_visible_to_team(TeamId::RED, true);
    app.viewport_mut().center_on_grid(32, 22);

    let (tx, ty) = screen_point(&app, Point::new(30, 22));
    let (ex, ey) = screen_point(&app, Point::new(34, 22));

    handle_session_input(&mut app, InputEvent::MouseClick(tx, ty));
    assert_eq!(app.hud().selected, Some(tank));

    handle_session_input(&mut app, InputEvent::MouseClick(ex, ey));

    let unit = app.state_ref().unit(tank).expect("Tank");
    assert_eq!(
        unit.orders,
        UnitOrder::MoveToAttack,
        "атакуемый враг — приказ атаки, а не осмотр"
    );
    assert_eq!(unit.enemy_unit, Some(enemy));
    assert_eq!(
        app.hud().selected,
        Some(tank),
        "свой юнит остаётся выделенным"
    );
}
