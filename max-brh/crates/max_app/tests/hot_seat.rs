//! M-HS: end-to-end hot-seat tests (2–4 humans on one PC).
//!
//! Checked:
//!
//! 1. setup: `ChoosePlayerMenu` in hot-seat mode does not copy the human
//!    flag, `EventDone` requires two humans (`_(06ec)`), the starting team
//!    types — `RED`/`GREEN` humans (`menu.cpp:1883`);
//! 2. turn transition: the `DialogMenu` window («<Team>: Begin turn.») opens on
//!    a human team change and for the **first** player too (C++ sets
//!    `GameManager_PlayerTeam = PLAYER_TEAM_ALIEN`), while the turn timer is stopped;
//! 3. control: the `player_team` switches to the active human,
//!    the selection is cleared, a click on a foreign unit does not select it, an order
//!    to a foreign unit does not go; during the AI turn input is blocked;
//! 4. turns: 2 humans alternate RED → GREEN; 4 humans — RED → GREEN →
//!    BLUE → GRAY; the AI between humans moves itself;
//! 5. frame: the transition window is drawn over the match (`HELPFRAM` background, the
//!    `HELPOK` button, the text "Begin turn") and does not panic.

use std::path::{Path, PathBuf};

use max_app::hotseat;
use max_app::local::{LocalPhase, LocalSetup};
use max_app::setup::{GameSetupForm, SetupScreen, SetupTransition};
use max_app::ui::{handle_session_input, InputEvent, MenuClient, Screen, UiContext};
use max_app::{App, AppConfig};
use max_core::enums::PlayMode;
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_net::{CreateGameParams, GameInfo};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};

struct NoNetwork;

impl MenuClient for NoNetwork {
    fn list_games(&mut self) -> Result<Vec<GameInfo>, String> {
        Err("offline".to_string())
    }

    fn create_game(&mut self, _params: CreateGameParams) -> Result<GameInfo, String> {
        Err("offline".to_string())
    }
}

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn local_app() -> Option<App> {
    let data = repo_path("max-original");

    if !data.join("MAX.RES").is_file() {
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(&data)
        .with_assets_dir(repo_path("tests/fixtures/assets"))
        .with_mode(max_app::AppMode::Ui);

    let mut app = App::new(config);
    app.init_assets().ok()?;

    Some(app)
}

/// Hot-seat settings: `team_types` — as the team screen would pass.
fn hot_seat_setup(team_types: [TeamKind; 4]) -> LocalSetup {
    LocalSetup {
        player_name: "Commander".to_string(),
        world: 5,
        opponent: 2,
        timer: 0,
        endturn: 0,
        play_mode: PlayMode::TurnBased,
        start_gold: 150,
        raw: 1,
        fuel: 1,
        gold: 1,
        derelicts: 0,
        victory_type: max_core::enums::VictoryType::Duration,
        victory_limit: 50,
        team_types,
        team_clans: [max_core::enums::TeamClan::Random; 4],
        hot_seat: true,
        seed: 13,
        start_positions: [None; 4],
        team_loadouts: std::array::from_fn(|_| None),
        team_gold: [None; 4],
        team_upgrades: std::array::from_fn(|_| None),
        team_gold_spent: [None; 4],
    }
}

/// Confirms the transition window if it is open.
fn acknowledge(app: &mut App) -> Option<TeamId> {
    let team = app.local_notice().map(|notice| notice.team);
    app.local_acknowledge_turn();
    team
}

/// Screen point of the cell center with 1:1 chrome (`WINDOW_MAIN_MAP` 180,18).
fn chrome_screen_of(app: &App, grid: Point) -> (i32, i32) {
    let (x, y) = max_app::hud::tile_center(&app.map_viewport(), grid);

    (
        x + max_render::layout::MAIN_MAP.ulx,
        y + max_render::layout::MAIN_MAP.uly,
    )
}

/// A free adjacent cell (without units) — the target of a move order.
fn free_neighbour(app: &App, grid: Point) -> Option<Point> {
    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1)] {
        let candidate = Point::new(grid.x + dx, grid.y + dy);

        if candidate.x < 0 || candidate.y < 0 || candidate.x >= 112 || candidate.y >= 112 {
            continue;
        }
        if app.state_ref().units_at(candidate).is_empty() {
            return Some(candidate);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// 1. Setup
// ---------------------------------------------------------------------------

#[test]
fn hot_seat_form_keeps_multiple_humans_and_titles_the_screen() {
    let mut form = GameSetupForm::hot_seat();

    // `menu_choose_player_menu_loop(false)`: RED and GREEN — humans.
    assert_eq!(
        form.setup.team_types,
        [
            TeamKind::Player,
            TeamKind::Player,
            TeamKind::None,
            TeamKind::None
        ]
    );
    assert!(form.setup.is_hot_seat());
    assert_eq!(form.players_title(), "Hot Seat Menu");

    // «Human» on BLUE does not take the flag from RED (`EventSelectHuman`).
    form.select_team_type(2, TeamKind::Player);
    assert_eq!(form.setup.team_types[0], TeamKind::Player);
    assert_eq!(form.setup.team_types[2], TeamKind::Player);
    assert_eq!(form.setup.human_count(), 3);

    // The single-player form still copies the flag.
    let mut single = GameSetupForm::default();
    single.setup.team_types = [
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::None,
        TeamKind::None,
    ];
    single.select_team_type(1, TeamKind::Player);
    assert_eq!(single.setup.team_types[0], TeamKind::Computer);
    assert_eq!(single.players_title(), "Custom Game Menu");
}

#[test]
fn hot_seat_requires_two_humans() {
    let mut form = GameSetupForm::hot_seat();
    form.screen = SetupScreen::Players;
    form.setup.team_types = [
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::None,
        TeamKind::None,
    ];

    let done = max_app::setup::PLAYER_BUTTONS[2].0;
    let transition =
        form.handle_input(InputEvent::MouseClick(done.ulx + 2, done.uly + 2), 640, 480);

    assert_eq!(transition, SetupTransition::None);
    assert_eq!(form.status, hotseat::MSG_TWO_HUMANS);

    // Two humans — the match starts.
    form.setup.team_types[1] = TeamKind::Player;
    let transition =
        form.handle_input(InputEvent::MouseClick(done.ulx + 2, done.uly + 2), 640, 480);

    match transition {
        SetupTransition::Start(setup) => {
            assert!(setup.is_hot_seat());
            assert_eq!(setup.human_teams(), vec![TeamId::RED, TeamId::GREEN]);
        }
        other => panic!("ожидался Start, получено {other:?}"),
    }
}

#[test]
fn hot_seat_forces_turn_based_mode() {
    let mut setup = hot_seat_setup([
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::None,
        TeamKind::None,
    ]);
    setup.play_mode = PlayMode::SimultaneousMoves;

    // `GameManager_UpdateHumanPlayerCount`: >1 human → turn-based.
    assert_eq!(setup.effective_play_mode(), PlayMode::TurnBased);

    setup.team_types[1] = TeamKind::Computer;
    assert_eq!(setup.effective_play_mode(), PlayMode::SimultaneousMoves);
}

// ---------------------------------------------------------------------------
// 2–4. Local loop: turn transition, control, alternation
// ---------------------------------------------------------------------------

#[test]
fn two_humans_alternate_with_the_begin_turn_dialog() {
    let mut app = local_app().expect(concat!("test fixture is missing: ", stringify!(local_app())));

    app.start_local_game(&hot_seat_setup([
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::None,
        TeamKind::None,
    ]))
    .expect("hot-seat партия стартует");

    // The first human is shown the window too (C++: `PlayerTeam = ALIEN` until
    // the first turn), although `player_team` is already switched to them.
    assert_eq!(
        app.local_notice().map(|notice| notice.team),
        Some(TeamId::RED)
    );
    assert_eq!(
        app.local().map(|local| local.phase),
        Some(LocalPhase::PlayerTurn)
    );
    assert_eq!(app.state_ref().player_team(), TeamId::RED);
    assert_eq!(app.state_ref().teams()[0].team_type, TeamKind::Player);
    assert_eq!(app.state_ref().teams()[1].team_type, TeamKind::Player);
    assert_eq!(app.state_ref().teams()[2].team_type, TeamKind::None);
    // Hot-seat is always turn-based.
    assert_eq!(app.state_ref().play_mode(), PlayMode::TurnBased);

    // The turn does not close until the window is confirmed.
    app.local_end_turn();
    assert_eq!(app.state_ref().turn_counter(), 1);
    assert_eq!(
        app.local_notice().map(|notice| notice.team),
        Some(TeamId::RED)
    );

    assert_eq!(acknowledge(&mut app), Some(TeamId::RED));
    assert!(app.local_notice().is_none());

    // RED: we move and close the turn → the GREEN window.
    app.local_end_turn();
    assert_eq!(
        app.local_notice().map(|notice| notice.team),
        Some(TeamId::GREEN)
    );
    assert_eq!(app.state_ref().player_team(), TeamId::GREEN, "управление у GREEN");
    assert_eq!(
        app.local().map(|local| local.active_team),
        Some(TeamId::GREEN)
    );

    assert_eq!(acknowledge(&mut app), Some(TeamId::GREEN));
    app.local_end_turn();

    // Both humans of the round have played — the turn counter grew, RED again.
    assert_eq!(app.state_ref().turn_counter(), 2);
    assert_eq!(
        app.local_notice().map(|notice| notice.team),
        Some(TeamId::RED)
    );
    assert_eq!(app.state_ref().player_team(), TeamId::RED);

    // Three more rounds in a row: the order and control switching are stable.
    for round in 2..=4 {
        for expected in [TeamId::RED, TeamId::GREEN] {
            assert_eq!(
                acknowledge(&mut app),
                Some(expected),
                "раунд {round}: окно перехода {expected:?}"
            );
            assert_eq!(app.state_ref().player_team(), expected);
            assert_eq!(
                app.local().map(|local| local.active_team),
                Some(expected),
                "раунд {round}: активная команда"
            );
            app.local_end_turn();
        }

        assert!(!app.local().is_some_and(|local| local.finished()));
        assert_eq!(app.state_ref().turn_counter(), round + 1, "раунд {round} закрыт");
    }
}

#[test]
fn four_humans_all_play_in_team_order() {
    let mut app = local_app().expect(concat!("test fixture is missing: ", stringify!(local_app())));

    app.start_local_game(&hot_seat_setup([
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::Player,
    ]))
    .expect("партия 4 человек");

    assert_eq!(app.local().map(|local| local.human_teams.len()), Some(4));

    let mut order = Vec::new();

    for _ in 0..4 {
        let notice = acknowledge(&mut app).expect("окно перехода очередного игрока");
        let active = app.local().map(|local| local.active_team).expect("партия");

        assert_eq!(notice, active, "окно принадлежит активной команде");
        assert_eq!(app.state_ref().player_team(), active, "управление у {active:?}");

        order.push(active);
        app.local_end_turn();
    }

    assert_eq!(
        order,
        vec![TeamId::RED, TeamId::GREEN, TeamId::BLUE, TeamId::GRAY]
    );
    assert_eq!(app.state_ref().turn_counter(), 2, "раунд закрылся один раз");
}

#[test]
fn ai_plays_between_two_humans() {
    let mut app = local_app().expect(concat!("test fixture is missing: ", stringify!(local_app())));

    // RED — human, GREEN — computer, BLUE — human.
    app.start_local_game(&hot_seat_setup([
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::Player,
        TeamKind::None,
    ]))
    .expect("партия с ИИ между людьми");

    acknowledge(&mut app);
    app.local_end_turn();

    // The computer turn RED → GREEN goes itself (`Ai_BeginTurn`).
    assert!(
        matches!(
            app.local().map(|local| local.phase),
            Some(LocalPhase::AiTurn(TeamId::GREEN))
        ),
        "после RED должен ходить компьютер GREEN"
    );

    let mut saw_ai = false;
    let mut notice = None;

    for _ in 0..40_000 {
        if matches!(
            app.local().map(|local| local.phase),
            Some(LocalPhase::AiTurn(_))
        ) {
            saw_ai = true;
        }
        if app.local_notice().is_some() {
            notice = app.local_notice().map(|notice| notice.team);
            break;
        }
        if app.local().is_none_or(|local| local.finished()) {
            break;
        }
        app.local_update(0.0);
    }

    assert!(saw_ai, "ИИ действительно ходил");
    assert_eq!(
        notice,
        Some(TeamId::BLUE),
        "после ИИ — окно второго человека"
    );
    assert_eq!(app.state_ref().player_team(), TeamId::BLUE);
    assert_eq!(app.state_ref().turn_counter(), 1, "раунд ещё не закрыт");
}

#[test]
fn notice_freezes_the_turn_timer_until_acknowledged() {
    let mut app = local_app().expect(concat!("test fixture is missing: ", stringify!(local_app())));

    let mut setup = hot_seat_setup([
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::None,
        TeamKind::None,
    ]);
    setup.timer = 5;
    app.start_local_game(&setup).expect("партия");

    let ticks_before = app.ticks();

    // The window is open: the frames do nothing (C++ sits in `DialogMenu::Run`).
    for _ in 0..10 {
        let update = app.local_update(1.0);
        assert!(!update.ticked);
    }

    assert_eq!(app.ticks(), ticks_before, "на окне перехода тиков нет");
    assert_eq!(app.local().map(|local| local.timer_left), Some(5.0));

    // After confirmation the timer runs and closes the turn itself.
    acknowledge(&mut app);
    app.local_update(1.0);
    assert!(
        app.local().map(|local| local.timer_left).unwrap_or(5.0) < 5.0,
        "таймер пошёл после подтверждения"
    );

    for _ in 0..10 {
        app.local_update(1.0);
    }

    assert_eq!(
        app.local_notice().map(|notice| notice.team),
        Some(TeamId::GREEN),
        "таймер закрыл ход RED и передал его GREEN"
    );
}

#[test]
fn only_the_active_human_controls_their_units() {
    let mut app = local_app().expect(concat!("test fixture is missing: ", stringify!(local_app())));

    app.start_local_game(&hot_seat_setup([
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::None,
        TeamKind::None,
    ]))
    .expect("партия");

    assert!(app.chrome_active(), "кадр 640×480 с хромом FRAMEPIC");
    acknowledge(&mut app);

    // We find a unit of each team and put the camera on the RED unit.
    let red_unit = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state_ref()
                .unit(*id)
                .is_some_and(|unit| unit.team == TeamId::RED)
        })
        .expect("юнит RED");
    let green_unit = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state_ref()
                .unit(*id)
                .is_some_and(|unit| unit.team == TeamId::GREEN)
        })
        .expect("юнит GREEN");
    let green_grid = app.state_ref().unit(green_unit).expect("юнит").grid;
    let red_grid = app.state_ref().unit(red_unit).expect("юнит").grid;

    app.viewport_mut()
        .center_on_grid(green_grid.x, green_grid.y);
    let (gx, gy) = chrome_screen_of(&app, green_grid);
    handle_session_input(&mut app, InputEvent::MouseClick(gx, gy));

    assert_eq!(app.hud().selected, None, "юнит чужой команды не выделяется");
    assert!(
        app.hud()
            .log
            .lines()
            .iter()
            .all(|line| !line.contains("Selected")),
        "в журнале нет выделения чужого юнита"
    );

    // An own unit is selected (several units stand on the start cell — what matters is
    // not the specific id but the team).
    app.viewport_mut().center_on_grid(red_grid.x, red_grid.y);
    let (rx, ry) = chrome_screen_of(&app, red_grid);
    handle_session_input(&mut app, InputEvent::MouseClick(rx, ry));

    let selected = app.hud().selected.expect("свой юнит выделен");
    assert_eq!(
        app.state_ref().unit(selected).map(|unit| unit.team),
        Some(TeamId::RED),
        "выделен юнит RED"
    );

    // A left click on a free cell — a move order to the selected unit
    // (`game_manager.cpp:6685-6745`: LMB is the order button); in demo mode
    // (`online = false`) it is applied immediately.
    let green_orders: Vec<_> = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state_ref().unit(id))
        .filter(|unit| unit.team == TeamId::GREEN)
        .map(|unit| unit.orders)
        .collect();

    if let Some(destination) = free_neighbour(&app, red_grid) {
        let (dx, dy) = chrome_screen_of(&app, destination);
        handle_session_input(&mut app, InputEvent::MouseClick(dx, dy));

        let ordered = app
            .state_ref()
            .unit(selected)
            .map(|unit| unit.orders)
            .expect("юнит RED");
        assert!(
            !matches!(
                ordered,
                max_core::enums::UnitOrder::Await | max_core::enums::UnitOrder::Idle
            ),
            "приказ ушёл выделенному своему юниту: {ordered:?}"
        );
    }

    let green_orders_after: Vec<_> = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state_ref().unit(id))
        .filter(|unit| unit.team == TeamId::GREEN)
        .map(|unit| unit.orders)
        .collect();
    assert_eq!(
        green_orders, green_orders_after,
        "приказы чужой команды не меняются"
    );

    // GREEN turn: the window, confirmation, the RED selection is cleared.
    app.local_end_turn();
    assert_eq!(
        app.local_notice().map(|notice| notice.team),
        Some(TeamId::GREEN)
    );
    acknowledge(&mut app);
    assert_eq!(app.state_ref().player_team(), TeamId::GREEN);
    assert_eq!(
        app.hud().selected,
        None,
        "выделение прошлого игрока не переезжает"
    );

    // A click on a RED unit now does not select it (it is foreign to GREEN).
    app.viewport_mut().center_on_grid(red_grid.x, red_grid.y);
    let (rx, ry) = chrome_screen_of(&app, red_grid);
    handle_session_input(&mut app, InputEvent::MouseClick(rx, ry));
    assert_eq!(app.hud().selected, None, "для GREEN юнит RED — чужой");
}

#[test]
fn input_is_ignored_during_the_ai_turn() {
    let mut app = local_app().expect(concat!("test fixture is missing: ", stringify!(local_app())));

    app.start_local_game(&hot_seat_setup([
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::Player,
        TeamKind::None,
    ]))
    .expect("партия");

    acknowledge(&mut app);
    app.local_end_turn();

    assert!(matches!(
        app.local().map(|local| local.phase),
        Some(LocalPhase::AiTurn(TeamId::GREEN))
    ));
    assert!(app.local_input_blocked());

    let red_unit = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state_ref()
                .unit(*id)
                .is_some_and(|unit| unit.team == TeamId::RED)
        })
        .expect("юнит RED");
    let grid = app.state_ref().unit(red_unit).expect("юнит").grid;

    app.viewport_mut().center_on_grid(grid.x, grid.y);
    let (x, y) = chrome_screen_of(&app, grid);
    handle_session_input(&mut app, InputEvent::MouseClick(x, y));

    assert_eq!(
        app.hud().selected,
        None,
        "во время хода ИИ выделение недоступно"
    );
    assert!(app.local_input_blocked());
}

#[test]
fn orders_are_blocked_while_the_transition_notice_is_open() {
    let mut app = local_app().expect(concat!("test fixture is missing: ", stringify!(local_app())));

    app.start_local_game(&hot_seat_setup([
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::None,
        TeamKind::None,
    ]))
    .expect("партия");

    assert!(app.local_notice().is_some());

    let red_unit = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state_ref()
                .unit(*id)
                .is_some_and(|unit| unit.team == TeamId::RED)
        })
        .expect("юнит RED");
    let grid = app.state_ref().unit(red_unit).expect("юнит").grid;

    app.viewport_mut().center_on_grid(grid.x, grid.y);
    let (x, y) = chrome_screen_of(&app, grid);
    handle_session_input(&mut app, InputEvent::MouseClick(x, y));

    assert_eq!(
        app.hud().selected,
        None,
        "окно модально: клик по карте — нет"
    );

    // Enter closes the window (`DialogMenu::ProcessKey`).
    handle_session_input(&mut app, InputEvent::Key(max_app::ui::Key::Enter));
    assert!(app.local_notice().is_none());

    handle_session_input(&mut app, InputEvent::MouseClick(x, y));
    let selected = app.hud().selected.expect("после OK ввод работает");
    assert_eq!(
        app.state_ref().unit(selected).map(|unit| unit.team),
        Some(TeamId::RED)
    );
}

// ---------------------------------------------------------------------------
// 5. Frame with the transition window
// ---------------------------------------------------------------------------

#[test]
fn begin_turn_dialog_renders_over_the_game_frame() {
    let mut app = local_app().expect(concat!("test fixture is missing: ", stringify!(local_app())));

    app.start_local_game(&hot_seat_setup([
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::None,
        TeamKind::None,
    ]))
    .expect("партия");

    let mut ctx = UiContext::new(Box::new(NoNetwork), 640, 480);
    ctx.status = app
        .local()
        .map(|local| local.status_line(app.state_ref().turn_counter()))
        .unwrap_or_default();

    let screen = Screen::InSession {
        request: max_app::SessionRequest {
            game_id: "local".to_string(),
            password: None,
        },
    };

    // A frame without the window: the match, the window is not confirmed yet — it should be there.
    let mut buffer = RgbaBuffer::new(640, 480);
    screen.render(&mut buffer, ctx.view(Some(&app)));

    let dialog = hotseat::dialog_rect_frame();
    assert_eq!(dialog, PixelRect::new(253, 126, 301, 231));

    // The `HELPFRAM` background is drawn: in the dialog window there are pixels different from
    // black, and they do not match the map frame under it.
    let painted = (dialog.ulx..dialog.ulx + dialog.width)
        .flat_map(|x| (dialog.uly..dialog.uly + dialog.height).map(move |y| (x, y)))
        .filter(|(x, y)| {
            buffer
                .get(*x, *y)
                .is_some_and(|color| color != Rgba::rgb(0, 0, 0))
        })
        .count();
    assert!(painted > 40_000, "окно HELPFRAM заполнено: {painted}");

    // The `HELPOK` button is drawn at (373,319) 65×24.
    let ok = hotseat::ok_rect_frame();
    let ok_pixels = (ok.ulx..ok.ulx + ok.width)
        .flat_map(|x| (ok.uly..ok.uly + ok.height).map(move |y| (x, y)))
        .filter(|(x, y)| buffer.get(*x, *y).is_some_and(|c| c != Rgba::rgb(0, 0, 0)))
        .count();
    assert!(ok_pixels > 500, "кнопка OK видна: {ok_pixels}");

    // The text `Begin turn` — light pixels of color 0xFF (`Fonts` palette).
    let palette = app
        .assets()
        .and_then(|assets| max_app::panels::UiPalette::from_assets(assets))
        .expect("палитра окна");
    let text_color = palette.rgba(hotseat::TEXT_COLOR);
    let text_pixels = (0..640)
        .flat_map(|x| (0..480).map(move |y| (x, y)))
        .filter(|(x, y)| buffer.get(*x, *y) == Some(text_color))
        .count();
    assert!(text_pixels > 30, "текст окна: {text_pixels} пикселей");

    // Outside the window the frame is the match (map/panels), not emptiness.
    assert!(buffer.get(10, 400).is_some());
}

#[test]
fn screens_and_setup_render_with_hot_seat_forms() {
    let app = local_app().expect(concat!("test fixture is missing: ", stringify!(local_app())));

    let ctx = UiContext::new(Box::new(NoNetwork), 640, 480);
    let mut buffer = RgbaBuffer::new(640, 480);

    let mut form = GameSetupForm::hot_seat();
    form.screen = SetupScreen::Players;
    form.setup.team_types = [
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::None,
    ];

    let screen = Screen::LocalSetup(form);
    screen.render(&mut buffer, ctx.view(Some(&app)));

    let menu = app
        .assets()
        .and_then(|assets| assets.menu.as_ref())
        .expect("меню загружено");

    // The `CHOSPLYR` background and the «Hot Seat Menu» title (pixels of `COLOR_GREEN`).
    assert_eq!(
        buffer.get(10, 10),
        menu.chosplyr
            .as_ref()
            .and_then(|image| image.rgb(10, 10))
            .map(Rgba::from_rgb)
    );

    let green = menu.palette.rgba(max_app::menu::GREEN);
    let title = max_app::setup::PLAYERS_TITLE;
    let title_pixels = (title.ulx..title.ulx + title.width)
        .flat_map(|x| (title.uly..title.uly + title.height).map(move |y| (x, y)))
        .filter(|(x, y)| buffer.get(*x, *y) == Some(green))
        .count();
    assert!(title_pixels > 20, "заголовок Hot Seat Menu: {title_pixels}");

    // `Multiplayer Menu` is drawn on `MAINPIC` without panics.
    let screen = Screen::MultiplayerMenu;
    screen.render(&mut buffer, ctx.view(Some(&app)));
    assert_eq!(
        buffer.get(10, 10),
        menu.mainpic.rgb(10, 10).map(Rgba::from_rgb)
    );

    // The title and buttons: the `Hot Seat Game` label lies at (385,280) 200×29.
    let button = max_app::menu::MULTIPLAYER_BUTTONS[2];
    assert_eq!(button.rect, PixelRect::new(385, 280, 200, 29));
    assert_eq!(button.label, "Hot Seat Game");
    let caption_pixels = (button.rect.ulx..button.rect.ulx + button.rect.width)
        .flat_map(|x| (button.rect.uly..button.rect.uly + button.rect.height).map(move |y| (x, y)))
        .filter(|(x, y)| buffer.get(*x, *y) == Some(menu.palette.rgba(165)))
        .count();
    assert!(caption_pixels > 20, "подпись кнопки: {caption_pixels}");
}

#[test]
fn begin_turn_message_names_the_team() {
    assert_eq!(
        hotseat::TurnNotice::new(TeamId::GRAY).lines(),
        vec!["Gray Team:", "Begin turn."]
    );
    assert_eq!(
        hotseat::TurnNotice::new(TeamId::RED).message(),
        "Red Team:\nBegin turn."
    );
}
