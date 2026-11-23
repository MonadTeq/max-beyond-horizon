//! Behavioral tests of the `Preferences` settings that previously were only
//! saved (item B4 of the `max-brh-docs/analysis/visual-1to1-plan.md` plan).
//!
//! Each setting is checked "on/off changes behavior":
//!
//! | Setting | Test |
//! |---|---|
//! | `click_scroll` | [`click_scroll_controls_whether_hover_scrolls`] |
//! | `quick_scroll` | [`quick_scroll_sets_the_camera_step`] |
//! | `fast_movement` | [`fast_movement_doubles_the_visual_unit_speed`] |
//! | `follow_unit` | [`follow_unit_centers_the_camera_on_the_selected_unit`] |
//! | `auto_select` | [`auto_select_picks_the_next_unit_after_a_move`] |
//! | `enemy_halt` | `max_core/tests/enemy_halt.rs` + [`enemy_halt_reaches_the_simulation`] |
//! | `timer`/`endturn` | [`turn_timers_from_preferences_apply_to_the_local_game`] |
//! | `player_name` | [`player_name_applies_to_the_game_and_the_v70_save`] |

use std::path::{Path, PathBuf};

use max_app::hud::build_move_order;
use max_app::local::LocalSetup;
use max_app::ui::{InputEvent, Key};
use max_app::{App, AppConfig};
use max_core::enums::{PlayMode, UnitOrder, UnitType, VictoryType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn temp_save_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("max-prefs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Application with real assets (the match needs a planet map).
fn local_app(save_dir: &Path) -> Option<App> {
    let data = repo_path("max-original");

    if !data.join("MAX.RES").is_file() {
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(&data)
        .with_assets_dir(repo_path("assets"))
        .with_mode(max_app::AppMode::Ui);

    let mut config = config;
    config.save_dir = Some(save_dir.to_path_buf());

    let mut app = App::new(config);
    app.init_assets().ok()?;

    Some(app)
}

fn setup_vs_one_ai() -> LocalSetup {
    LocalSetup {
        player_name: "Alice".to_string(),
        world: 5,
        opponent: 2,
        timer: 180,
        endturn: 45,
        play_mode: PlayMode::TurnBased,
        start_gold: 150,
        raw: 1,
        fuel: 1,
        gold: 1,
        derelicts: 0,
        victory_type: VictoryType::Duration,
        victory_limit: 50,
        team_types: [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ],
        team_clans: [max_core::enums::TeamClan::Random; 4],
        hot_seat: false,
        seed: 7,
        start_positions: [None; 4],
        team_loadouts: std::array::from_fn(|_| None),
        team_gold: [None; 4],
        team_upgrades: std::array::from_fn(|_| None),
        team_gold_spent: [None; 4],
    }
}

/// The first team unit with movement points (`speed > 0`) — it can be ordered
/// to move this turn.
fn mobile_unit(app: &App) -> Option<max_core::unit::UnitId> {
    let team = app.state_ref().player_team();

    app.state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state_ref().unit(*id).is_some_and(|unit| {
                unit.team == team
                    && unit.speed > 0
                    && matches!(
                        unit.unit_type,
                        UnitType::Constructor | UnitType::Engineer | UnitType::Surveyor | UnitType::Tank
                    )
            })
        })
}

// ---------------------------------------------------------------------------
// 1. click_scroll / quick_scroll — camera
// ---------------------------------------------------------------------------

#[test]
fn click_scroll_controls_whether_hover_scrolls() {
    let mut app = App::new(AppConfig::default());
    app.viewport_mut().center_on_grid(50, 50);

    let mut prefs = app.preferences().clone();
    prefs.click_scroll = false;
    prefs.quick_scroll = 16;
    app.set_preferences(prefs);

    // Hovering the top scroll strip (window 50..590 × 0..8).
    app.note_mouse_move(300, 4);

    let before = app.viewport().center();
    let step = app.update_scroll(1.0 / 30.0);

    assert_eq!(step, Some((0, -16)), "клик-скролл выключен: скроллит наведение");
    assert_eq!(
        app.viewport().center(),
        (before.0, before.1 - 16),
        "камера уехала вверх на quick_scroll пикселей"
    );

    // Enable "Click to Scroll": hovering no longer scrolls.
    let mut prefs = app.preferences().clone();
    prefs.click_scroll = true;
    app.set_preferences(prefs);

    let before = app.viewport().center();
    assert_eq!(app.update_scroll(1.0 / 30.0), None, "без нажатой ЛКМ скролла нет");
    assert_eq!(app.viewport().center(), before);

    // A click on the strip — a "hold", like `GameManager_SetGridOffset`.
    app.note_mouse_click(300, 4);
    assert_eq!(
        app.update_scroll(1.0 / 30.0),
        Some((0, -16)),
        "клик по верхней полосе прокручивает карту"
    );

    // Panel buttons are more important than scrolling (in C++ `WINDOW_ENDTURN_BUTTON` before
    // `WINDOW_SCROLL_UP_WINDOW`): End Turn (390,2) 72×21.
    let before = app.viewport().center();
    app.note_mouse_move(400, 4);
    app.note_mouse_click(400, 4);
    assert_eq!(app.update_scroll(1.0 / 30.0), None);
    assert_eq!(app.viewport().center(), before);

    // Outside the strips there is no scrolling.
    app.note_mouse_move(300, 200);
    assert_eq!(app.update_scroll(1.0 / 30.0), None);
}

#[test]
fn quick_scroll_sets_the_camera_step() {
    let mut app = App::new(AppConfig::default());

    for (speed, expected) in [(16, 16), (40, 40), (64, 64), (128, 128)] {
        app.viewport_mut().center_on_grid(50, 50);
        let mut prefs = app.preferences().clone();
        prefs.click_scroll = false;
        prefs.quick_scroll = speed;
        app.set_preferences(prefs);
        app.note_mouse_move(636, 200);

        assert_eq!(
            app.update_scroll(1.0 / 30.0),
            Some((expected, 0)),
            "Scroll Speed {speed} → шаг {expected} px/кадр (30 Гц)"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. fast_movement — sprite speed
// ---------------------------------------------------------------------------

#[test]
fn fast_movement_doubles_the_visual_unit_speed() {
    let mut slow = local_app(&temp_save_dir("fast-off"));
    let mut fast = local_app(&temp_save_dir("fast-on"));

    let (Some(mut slow), Some(mut fast)) = (slow.take(), fast.take()) else {
        return;
    };

    let setup = setup_vs_one_ai();
    // The setting is set before the start, like the main menu Preferences.
    let mut prefs = slow.preferences().clone();
    prefs.fast_movement = false;
    slow.set_preferences(prefs);
    let mut prefs = fast.preferences().clone();
    prefs.fast_movement = true;
    fast.set_preferences(prefs);

    slow.start_local_game(&setup).expect("партия (off)");
    fast.start_local_game(&setup).expect("партия (on)");

    let id = mobile_unit(&slow).expect("мобильный юнит");
    assert!(mobile_unit(&fast).is_some());

    // The same order: an adjacent passable cell.
    let from = slow.state_ref().unit(id).expect("юнит").grid;
    let mut destination = None;

    'search: for dy in [-1i16, 0, 1] {
        for dx in [-1i16, 0, 1] {
            if dx == 0 && dy == 0 {
                continue;
            }

            let target = Point::new(from.x + dx, from.y + dy);

            if build_move_order(slow.state_ref(), id, target).is_some() {
                destination = Some(target);
                break 'search;
            }
        }
    }

    let destination = destination.expect("рядом есть проходимая клетка");

    for app in [&mut slow, &mut fast] {
        let order = build_move_order(app.state_ref(), id, destination).expect("приказ");
        app.apply(order.command);
        app.update_visual_units(0.0);
    }

    let start = slow.visual_position(id).expect("юнит");

    // The frame passes the route: the simulation ticks (a cell per tick), the sprite
    // catches up with it at real speed. 10 frames at 60 Hz — 1/6 s.
    for _ in 0..10 {
        slow.local_update(1.0 / 60.0);
        fast.local_update(1.0 / 60.0);
    }

    let distance = |app: &App| {
        let (x, y) = app.visual_position(id).expect("юнит");
        ((x - start.0).powi(2) + (y - start.1).powi(2)).sqrt()
    };

    let slow_distance = distance(&slow);
    let fast_distance = distance(&fast);

    assert!(
        slow_distance > 0.0,
        "спрайт должен двигаться: прошёл {slow_distance}"
    );
    assert!(
        fast_distance > slow_distance * 1.6,
        "fast_movement удваивает скорость спрайта (C++ `UnitInfo::Move`): \
         off={slow_distance}, on={fast_distance}"
    );
}

// ---------------------------------------------------------------------------
// 3. follow_unit — the camera follows the selection
// ---------------------------------------------------------------------------

#[test]
fn follow_unit_centers_the_camera_on_the_selected_unit() {
    let dir = temp_save_dir("follow");
    let Some(mut app) = local_app(&dir) else {
        return;
    };

    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия");

    let id = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state_ref()
                .unit(*id)
                .is_some_and(|unit| unit.team == TeamId::RED)
        })
        .expect("юнит игрока");
    let grid = app.state_ref().unit(id).expect("юнит").grid;

    app.select_unit(Some(id));
    // Move the camera far away so that the unit is outside the map window.
    app.viewport_mut().center_on_grid(5, 5);

    let mut prefs = app.preferences().clone();
    prefs.follow_unit = false;
    app.set_preferences(prefs);
    let before = app.viewport().center();
    assert!(!app.follow_selected_unit(), "follow_unit выключен");
    assert_eq!(app.viewport().center(), before);

    let mut prefs = app.preferences().clone();
    prefs.follow_unit = true;
    app.set_preferences(prefs);

    assert!(app.follow_selected_unit(), "камера должна догнать юнита");
    assert_eq!(
        app.viewport().center(),
        (
            i32::from(grid.x) * 64 + 32,
            i32::from(grid.y) * 64 + 32
        ),
        "центр камеры — клетка выделенного юнита (`UnitInfo::FollowUnit`)"
    );

    // The unit is already in the map window — the camera does not move again.
    assert!(!app.follow_selected_unit());
}

// ---------------------------------------------------------------------------
// 4. auto_select — the next unit after an order
// ---------------------------------------------------------------------------

#[test]
fn auto_select_picks_the_next_unit_after_a_move() {
    let dir = temp_save_dir("autoselect");
    let Some(mut app) = local_app(&dir) else {
        return;
    };

    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия");

    // --- off: the selection remains on the unit after the order completes ---
    let mut prefs = app.preferences().clone();
    prefs.auto_select = false;
    app.set_preferences(prefs);

    let id = mobile_unit(&app).expect("мобильный юнит");
    app.select_unit(Some(id));
    issue_short_move(&mut app, id);
    finish_path_next_tick(&mut app, id);
    run_until_idle(&mut app, id);

    assert_eq!(
        app.hud().selected,
        Some(id),
        "auto_select выключен — выделение не меняется"
    );

    // --- on: after the order completes the next unit is selected ---
    let mut prefs = app.preferences().clone();
    prefs.auto_select = true;
    app.set_preferences(prefs);

    let next_mover = mobile_unit(&app).expect("ещё один мобильный юнит");
    app.select_unit(Some(next_mover));
    issue_short_move(&mut app, next_mover);
    finish_path_next_tick(&mut app, next_mover);
    run_until_idle(&mut app, next_mover);

    let selected = app.hud().selected;
    assert_ne!(
        selected,
        Some(next_mover),
        "auto_select: после приказа выделяется следующий юнит"
    );
    assert!(
        selected
            .and_then(|id| app.state_ref().unit(id))
            .is_some_and(|unit| unit.team == TeamId::RED),
        "новое выделение — юнит игрока, а не пусто: {selected:?}"
    );
}

fn issue_short_move(app: &mut App, id: max_core::unit::UnitId) {
    let from = app.state_ref().unit(id).expect("юнит").grid;
    let mut destination = None;

    'search: for dy in [-1i16, 0, 1] {
        for dx in [-1i16, 0, 1] {
            if dx == 0 && dy == 0 {
                continue;
            }

            let target = Point::new(from.x + dx, from.y + dy);

            if build_move_order(app.state_ref(), id, target).is_some() {
                destination = Some(target);
                break 'search;
            }
        }
    }

    let destination = destination.expect("рядом есть проходимая клетка");
    let order = build_move_order(app.state_ref(), id, destination).expect("приказ");

    app.apply(order.command);
    app.update_visual_units(0.0);
}

/// Puts the unit into the "last route step, movement points exhausted" state.
///
/// This is how an order completion looks in C++: `GroundPath::Execute` on an exhausted
/// step calls `BlockedOnPathRequest` → `MoveFinished` → `AutoSelectNext`, and
/// `GameManager_IsInteractable` (`game_manager.cpp:3241`) at `speed == 0`
/// no longer keeps the selection on the unit.
fn finish_path_next_tick(app: &mut App, id: max_core::unit::UnitId) {
    // One tick — so that the simulation issues the path (`Paths_RequestPath`).
    app.local_update(1.0 / 60.0);

    if let Some(mut unit) = app.state_ref().unit(id).cloned() {
        if let Some(max_core::unit::UnitPath::Ground(path)) = unit.path.as_mut() {
            path.step_index = path.steps.len() as u32;
        }
        unit.speed = 0;
        app.universe_mut().write_unit(id, unit);
    }
}

fn run_until_idle(app: &mut App, id: max_core::unit::UnitId) {    for _ in 0..600 {
        let done = app.state_ref().unit(id).is_none_or(|unit| {
            matches!(unit.orders, UnitOrder::Await | UnitOrder::Idle) && unit.path.is_none()
        });

        if done {
            // Two more frames: `auto_select_after_tick` runs in the order
            // completion tick.
            app.local_update(1.0 / 60.0);
            app.local_update(1.0 / 60.0);
            return;
        }

        app.local_update(1.0 / 60.0);
    }

    panic!("приказ не завершился за 600 кадров");
}

// ---------------------------------------------------------------------------
// 5. enemy_halt — the setting reaches the simulation
// ---------------------------------------------------------------------------

#[test]
fn enemy_halt_reaches_the_simulation() {
    let dir = temp_save_dir("enemy-halt");
    let Some(mut app) = local_app(&dir) else {
        return;
    };

    let mut prefs = app.preferences().clone();
    prefs.enemy_halt = false;
    app.set_preferences(prefs);
    assert!(!app.state_ref().enemy_halt(), "выключено в Preferences");

    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия");
    assert!(
        !app.state_ref().enemy_halt(),
        "старт партии берёт enemy_halt из Preferences (settings.cpp)"
    );

    let mut prefs = app.preferences().clone();
    prefs.enemy_halt = true;
    app.set_preferences(prefs);
    assert!(
        app.state_ref().enemy_halt(),
        "изменение в игре применяется сразу (настройка читается на шаге)"
    );
}

// ---------------------------------------------------------------------------
// 6. Turn Time / End Turn Time
// ---------------------------------------------------------------------------

#[test]
fn turn_timers_from_preferences_apply_to_the_local_game() {
    let dir = temp_save_dir("timers");
    let Some(mut app) = local_app(&dir) else {
        return;
    };

    let setup = setup_vs_one_ai();
    app.start_local_game(&setup).expect("локальная партия");

    // `GameConfigMenu` `Done` writes the timers into the same `settings`
    // (`gameconfigmenu.cpp:134-141`), so the match start synchronizes them.
    assert_eq!(app.preferences().turn_timer, 180);
    assert_eq!(app.preferences().endturn, 45);
    assert_eq!(app.local().expect("партия").timer_left, 180.0);

    // Preferences in game — from the next turn (`GameManager_UpdateGui`).
    let mut prefs = app.preferences().clone();
    prefs.turn_timer = 60;
    prefs.endturn = 50;
    app.set_preferences(prefs);
    assert_eq!(app.local().expect("партия").setup.timer, 60);
    assert_eq!(app.local().expect("партия").setup.endturn, 50);

    app.local_end_turn();

    for _ in 0..20_000 {
        if app
            .local()
            .is_some_and(|local| local.phase == max_app::local::LocalPhase::PlayerTurn)
        {
            break;
        }

        app.local_update(0.0);
    }

    let local = app.local().expect("партия");
    assert_eq!(local.phase, max_app::local::LocalPhase::PlayerTurn);
    assert_eq!(
        local.timer_left, 60.0,
        "новый таймер хода из Preferences (C++ читает settings в UpdateGui)"
    );
}

// ---------------------------------------------------------------------------
// 7. Player Name
// ---------------------------------------------------------------------------

#[test]
fn player_name_applies_to_the_game_and_the_v70_save() {
    let dir = temp_save_dir("player-name");
    let Some(mut app) = local_app(&dir) else {
        return;
    };

    let mut setup = setup_vs_one_ai();
    setup.player_name = "Alice".to_string();
    app.start_local_game(&setup).expect("локальная партия");

    // The name is already in the HUD log (`Local game: <name> vs …`).
    assert!(
        app.hud()
            .log
            .lines()
            .iter()
            .any(|line| line.contains("Alice")),
        "имя игрока в журнале: {:?}",
        app.hud().log.lines()
    );

    // In game `PREFSPIC` shows the team name (`menu_team_name_setting`).
    assert!(app.open_preferences());
    assert_eq!(
        app.preferences_dialog().expect("диалог").draft.player_name,
        "Alice"
    );

    app.preferences_dialog_mut().expect("диалог").draft.player_name = "Bob".to_string();
    let action = app.preferences_input(InputEvent::Key(Key::Enter));

    assert_eq!(
        action,
        Some(max_app::preferences::PreferencesAction::Done)
    );
    assert_eq!(app.preferences().player_name, "Bob");
    assert_eq!(
        app.local().expect("партия").display_name(TeamId::RED),
        "Bob",
        "OptionsMenu PREFSPIC переименовывает команду сразу"
    );
    assert_eq!(
        max_app::winloss::display_name(app.local().expect("партия"), TeamId::RED),
        "Bob",
        "экран победы показывает имя из настроек (`menu.cpp:189`)"
    );

    // V70 save: `SaveLoad_Save` writes `menu_team_name_setting` into the header.
    let bytes = app.save_v70_bytes("Bob save").expect("V70-сейв");
    let save = max_core::save::SaveFile::parse(&bytes).expect("V70 разбирается");
    assert_eq!(save.header.team_name(0), "Bob");
    assert_eq!(save.header.turn_timer_time, 180);
}
