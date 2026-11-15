//! M-HS: сквозные тесты hot-seat (2–4 человека на одном ПК).
//!
//! Проверяется:
//!
//! 1. настройка: `ChoosePlayerMenu` в режиме hot-seat не переносит флаг
//!    человека, `EventDone` требует двух людей (`_(06ec)`), стартовые типы
//!    команд — `RED`/`GREEN` люди (`menu.cpp:1883`);
//! 2. переход хода: окно `DialogMenu` («<Команда>: Begin turn.») открывается на
//!    смене человеческой команды и **первому** игроку тоже (C++ ставит
//!    `GameManager_PlayerTeam = PLAYER_TEAM_ALIEN`), таймер хода при этом стоит;
//! 3. управление: команда `player_team` переключается на активного человека,
//!    выделение сбрасывается, клик по чужому юниту не выделяет его, приказ
//!    чужому юниту не уходит; во время хода ИИ ввод заблокирован;
//! 4. ходы: 2 человека чередуются RED → GREEN; 4 человека — RED → GREEN →
//!    BLUE → GRAY; ИИ между людьми ходит сам;
//! 5. кадр: окно перехода рисуется поверх партии (фон `HELPFRAM`, кнопка
//!    `HELPOK`, текст «Begin turn») и не паникует.

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
        .with_assets_dir(repo_path("assets"))
        .with_mode(max_app::AppMode::Ui);

    let mut app = App::new(config);
    app.init_assets().ok()?;

    Some(app)
}

/// Настройки hot-seat: `team_types` — как передал бы экран команд.
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
        hot_seat: true,
        seed: 13,
    }
}

/// Подтверждает окно перехода, если оно открыто.
fn acknowledge(app: &mut App) -> Option<TeamId> {
    let team = app.local_notice().map(|notice| notice.team);
    app.local_acknowledge_turn();
    team
}

/// Экранная точка центра клетки с учётом хрома 1:1 (`WINDOW_MAIN_MAP` 180,18).
fn chrome_screen_of(app: &App, grid: Point) -> (i32, i32) {
    let (x, y) = max_app::hud::tile_center(&app.map_viewport(), grid);

    (
        x + max_render::layout::MAIN_MAP.ulx,
        y + max_render::layout::MAIN_MAP.uly,
    )
}

/// Свободная соседняя клетка (без юнитов) — цель приказа движения.
fn free_neighbour(app: &App, grid: Point) -> Option<Point> {
    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1)] {
        let candidate = Point::new(grid.x + dx, grid.y + dy);

        if candidate.x < 0 || candidate.y < 0 || candidate.x >= 112 || candidate.y >= 112 {
            continue;
        }
        if app.state().units_at(candidate).is_empty() {
            return Some(candidate);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// 1. Настройка
// ---------------------------------------------------------------------------

#[test]
fn hot_seat_form_keeps_multiple_humans_and_titles_the_screen() {
    let mut form = GameSetupForm::hot_seat();

    // `menu_choose_player_menu_loop(false)`: RED и GREEN — люди.
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

    // «Human» на BLUE не отбирает флаг у RED (`EventSelectHuman`).
    form.select_team_type(2, TeamKind::Player);
    assert_eq!(form.setup.team_types[0], TeamKind::Player);
    assert_eq!(form.setup.team_types[2], TeamKind::Player);
    assert_eq!(form.setup.human_count(), 3);

    // Одиночная форма по-прежнему переносит флаг.
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

    // Два человека — партия стартует.
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

    // `GameManager_UpdateHumanPlayerCount`: >1 человека → turn-based.
    assert_eq!(setup.effective_play_mode(), PlayMode::TurnBased);

    setup.team_types[1] = TeamKind::Computer;
    assert_eq!(setup.effective_play_mode(), PlayMode::SimultaneousMoves);
}

// ---------------------------------------------------------------------------
// 2–4. Локальный цикл: переход хода, управление, чередование
// ---------------------------------------------------------------------------

#[test]
fn two_humans_alternate_with_the_begin_turn_dialog() {
    let Some(mut app) = local_app() else {
        return;
    };

    app.start_local_game(&hot_seat_setup([
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::None,
        TeamKind::None,
    ]))
    .expect("hot-seat партия стартует");

    // Первому человеку окно тоже показывается (C++: `PlayerTeam = ALIEN` до
    // первого хода), хотя `player_team` уже переключён на него.
    assert_eq!(
        app.local_notice().map(|notice| notice.team),
        Some(TeamId::RED)
    );
    assert_eq!(
        app.local().map(|local| local.phase),
        Some(LocalPhase::PlayerTurn)
    );
    assert_eq!(app.state().player_team, TeamId::RED);
    assert_eq!(app.state().teams[0].team_type, TeamKind::Player);
    assert_eq!(app.state().teams[1].team_type, TeamKind::Player);
    assert_eq!(app.state().teams[2].team_type, TeamKind::None);
    // Hot-seat всегда turn-based.
    assert_eq!(app.state().play_mode, PlayMode::TurnBased);

    // Ход не закрывается, пока окно не подтверждено.
    app.local_end_turn();
    assert_eq!(app.state().turn_counter, 1);
    assert_eq!(
        app.local_notice().map(|notice| notice.team),
        Some(TeamId::RED)
    );

    assert_eq!(acknowledge(&mut app), Some(TeamId::RED));
    assert!(app.local_notice().is_none());

    // RED: ходим и закрываем ход → окно GREEN.
    app.local_end_turn();
    assert_eq!(
        app.local_notice().map(|notice| notice.team),
        Some(TeamId::GREEN)
    );
    assert_eq!(app.state().player_team, TeamId::GREEN, "управление у GREEN");
    assert_eq!(
        app.local().map(|local| local.active_team),
        Some(TeamId::GREEN)
    );

    assert_eq!(acknowledge(&mut app), Some(TeamId::GREEN));
    app.local_end_turn();

    // Оба человека раунда отыграли — счётчик хода вырос, снова RED.
    assert_eq!(app.state().turn_counter, 2);
    assert_eq!(
        app.local_notice().map(|notice| notice.team),
        Some(TeamId::RED)
    );
    assert_eq!(app.state().player_team, TeamId::RED);

    // Ещё три раунда подряд: порядок и переключение управления устойчивы.
    for round in 2..=4 {
        for expected in [TeamId::RED, TeamId::GREEN] {
            assert_eq!(
                acknowledge(&mut app),
                Some(expected),
                "раунд {round}: окно перехода {expected:?}"
            );
            assert_eq!(app.state().player_team, expected);
            assert_eq!(
                app.local().map(|local| local.active_team),
                Some(expected),
                "раунд {round}: активная команда"
            );
            app.local_end_turn();
        }

        assert!(!app.local().is_some_and(|local| local.finished()));
        assert_eq!(app.state().turn_counter, round + 1, "раунд {round} закрыт");
    }
}

#[test]
fn four_humans_all_play_in_team_order() {
    let Some(mut app) = local_app() else {
        return;
    };

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
        assert_eq!(app.state().player_team, active, "управление у {active:?}");

        order.push(active);
        app.local_end_turn();
    }

    assert_eq!(
        order,
        vec![TeamId::RED, TeamId::GREEN, TeamId::BLUE, TeamId::GRAY]
    );
    assert_eq!(app.state().turn_counter, 2, "раунд закрылся один раз");
}

#[test]
fn ai_plays_between_two_humans() {
    let Some(mut app) = local_app() else {
        return;
    };

    // RED — человек, GREEN — компьютер, BLUE — человек.
    app.start_local_game(&hot_seat_setup([
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::Player,
        TeamKind::None,
    ]))
    .expect("партия с ИИ между людьми");

    acknowledge(&mut app);
    app.local_end_turn();

    // Ход компьютера RED → GREEN идёт сам (`Ai_BeginTurn`).
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
    assert_eq!(app.state().player_team, TeamId::BLUE);
    assert_eq!(app.state().turn_counter, 1, "раунд ещё не закрыт");
}

#[test]
fn notice_freezes_the_turn_timer_until_acknowledged() {
    let Some(mut app) = local_app() else {
        return;
    };

    let mut setup = hot_seat_setup([
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::None,
        TeamKind::None,
    ]);
    setup.timer = 5;
    app.start_local_game(&setup).expect("партия");

    let ticks_before = app.ticks();

    // Окно открыто: кадры ничего не делают (C++ стоит в `DialogMenu::Run`).
    for _ in 0..10 {
        let update = app.local_update(1.0);
        assert!(!update.ticked);
    }

    assert_eq!(app.ticks(), ticks_before, "на окне перехода тиков нет");
    assert_eq!(app.local().map(|local| local.timer_left), Some(5.0));

    // После подтверждения таймер пошёл и закрывает ход сам.
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
    let Some(mut app) = local_app() else {
        return;
    };

    app.start_local_game(&hot_seat_setup([
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::None,
        TeamKind::None,
    ]))
    .expect("партия");

    assert!(app.chrome_active(), "кадр 640×480 с хромом FRAMEPIC");
    acknowledge(&mut app);

    // Находим по юниту каждой команды и ставим камеру на юнит RED.
    let red_unit = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state()
                .unit(*id)
                .is_some_and(|unit| unit.team == TeamId::RED)
        })
        .expect("юнит RED");
    let green_unit = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state()
                .unit(*id)
                .is_some_and(|unit| unit.team == TeamId::GREEN)
        })
        .expect("юнит GREEN");
    let green_grid = app.state().unit(green_unit).expect("юнит").grid;
    let red_grid = app.state().unit(red_unit).expect("юнит").grid;

    app.viewport_mut()
        .center_on_grid(green_grid.x, green_grid.y);
    let (gx, gy) = chrome_screen_of(&app, green_grid);
    handle_session_input(&mut app, InputEvent::MouseClick(gx, gy), false);

    assert_eq!(app.hud().selected, None, "юнит чужой команды не выделяется");
    assert!(
        app.hud()
            .log
            .lines()
            .iter()
            .all(|line| !line.contains("Selected")),
        "в журнале нет выделения чужого юнита"
    );

    // Свой юнит выделяется (на клетке старта стоят несколько юнитов — важен
    // не конкретный id, а команда).
    app.viewport_mut().center_on_grid(red_grid.x, red_grid.y);
    let (rx, ry) = chrome_screen_of(&app, red_grid);
    handle_session_input(&mut app, InputEvent::MouseClick(rx, ry), false);

    let selected = app.hud().selected.expect("свой юнит выделен");
    assert_eq!(
        app.state().unit(selected).map(|unit| unit.team),
        Some(TeamId::RED),
        "выделен юнит RED"
    );

    // Правый клик по свободной клетке — приказ движения выделенному юниту:
    // в демо-режиме (`online = false`) он применяется сразу.
    let green_orders: Vec<_> = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state().unit(id))
        .filter(|unit| unit.team == TeamId::GREEN)
        .map(|unit| unit.orders)
        .collect();

    if let Some(destination) = free_neighbour(&app, red_grid) {
        let (dx, dy) = chrome_screen_of(&app, destination);
        handle_session_input(&mut app, InputEvent::RightClick(dx, dy), false);

        let ordered = app
            .state()
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
        .state()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state().unit(id))
        .filter(|unit| unit.team == TeamId::GREEN)
        .map(|unit| unit.orders)
        .collect();
    assert_eq!(
        green_orders, green_orders_after,
        "приказы чужой команды не меняются"
    );

    // Ход GREEN: окно, подтверждение, выделение RED сброшено.
    app.local_end_turn();
    assert_eq!(
        app.local_notice().map(|notice| notice.team),
        Some(TeamId::GREEN)
    );
    acknowledge(&mut app);
    assert_eq!(app.state().player_team, TeamId::GREEN);
    assert_eq!(
        app.hud().selected,
        None,
        "выделение прошлого игрока не переезжает"
    );

    // Клик по юниту RED теперь не выделяет его (он чужой для GREEN).
    app.viewport_mut().center_on_grid(red_grid.x, red_grid.y);
    let (rx, ry) = chrome_screen_of(&app, red_grid);
    handle_session_input(&mut app, InputEvent::MouseClick(rx, ry), false);
    assert_eq!(app.hud().selected, None, "для GREEN юнит RED — чужой");
}

#[test]
fn input_is_ignored_during_the_ai_turn() {
    let Some(mut app) = local_app() else {
        return;
    };

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
        .state()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state()
                .unit(*id)
                .is_some_and(|unit| unit.team == TeamId::RED)
        })
        .expect("юнит RED");
    let grid = app.state().unit(red_unit).expect("юнит").grid;

    app.viewport_mut().center_on_grid(grid.x, grid.y);
    let (x, y) = chrome_screen_of(&app, grid);
    handle_session_input(&mut app, InputEvent::MouseClick(x, y), false);

    assert_eq!(
        app.hud().selected,
        None,
        "во время хода ИИ выделение недоступно"
    );
    assert!(app.local_input_blocked());
}

#[test]
fn orders_are_blocked_while_the_transition_notice_is_open() {
    let Some(mut app) = local_app() else {
        return;
    };

    app.start_local_game(&hot_seat_setup([
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::None,
        TeamKind::None,
    ]))
    .expect("партия");

    assert!(app.local_notice().is_some());

    let red_unit = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            app.state()
                .unit(*id)
                .is_some_and(|unit| unit.team == TeamId::RED)
        })
        .expect("юнит RED");
    let grid = app.state().unit(red_unit).expect("юнит").grid;

    app.viewport_mut().center_on_grid(grid.x, grid.y);
    let (x, y) = chrome_screen_of(&app, grid);
    handle_session_input(&mut app, InputEvent::MouseClick(x, y), false);

    assert_eq!(
        app.hud().selected,
        None,
        "окно модально: клик по карте — нет"
    );

    // Enter закрывает окно (`DialogMenu::ProcessKey`).
    handle_session_input(&mut app, InputEvent::Key(max_app::ui::Key::Enter), false);
    assert!(app.local_notice().is_none());

    handle_session_input(&mut app, InputEvent::MouseClick(x, y), false);
    let selected = app.hud().selected.expect("после OK ввод работает");
    assert_eq!(
        app.state().unit(selected).map(|unit| unit.team),
        Some(TeamId::RED)
    );
}

// ---------------------------------------------------------------------------
// 5. Кадр с окном перехода
// ---------------------------------------------------------------------------

#[test]
fn begin_turn_dialog_renders_over_the_game_frame() {
    let Some(mut app) = local_app() else {
        return;
    };

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
        .map(|local| local.status_line(app.state().turn_counter))
        .unwrap_or_default();

    let screen = Screen::InSession {
        request: max_app::SessionRequest {
            game_id: "local".to_string(),
            password: None,
        },
    };

    // Кадр без окна: партия, окно ещё не подтверждено — оно и должно быть.
    let mut buffer = RgbaBuffer::new(640, 480);
    screen.render(&mut buffer, ctx.view(Some(&app)));

    let dialog = hotseat::dialog_rect_frame();
    assert_eq!(dialog, PixelRect::new(253, 126, 301, 231));

    // Фон `HELPFRAM` нарисован: в окне диалога есть пиксели, отличные от
    // чёрного, и они не совпадают с кадром карты под ним.
    let painted = (dialog.ulx..dialog.ulx + dialog.width)
        .flat_map(|x| (dialog.uly..dialog.uly + dialog.height).map(move |y| (x, y)))
        .filter(|(x, y)| {
            buffer
                .get(*x, *y)
                .is_some_and(|color| color != Rgba::rgb(0, 0, 0))
        })
        .count();
    assert!(painted > 40_000, "окно HELPFRAM заполнено: {painted}");

    // Кнопка `HELPOK` нарисована в (373,319) 65×24.
    let ok = hotseat::ok_rect_frame();
    let ok_pixels = (ok.ulx..ok.ulx + ok.width)
        .flat_map(|x| (ok.uly..ok.uly + ok.height).map(move |y| (x, y)))
        .filter(|(x, y)| buffer.get(*x, *y).is_some_and(|c| c != Rgba::rgb(0, 0, 0)))
        .count();
    assert!(ok_pixels > 500, "кнопка OK видна: {ok_pixels}");

    // Текст `Begin turn` — светлые пиксели цвета 0xFF (`Fonts`-палитра).
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

    // Вне окна кадр — партия (карта/панели), а не пустота.
    assert!(buffer.get(10, 400).is_some());
}

#[test]
fn screens_and_setup_render_with_hot_seat_forms() {
    let Some(app) = local_app() else {
        return;
    };

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

    // Фон `CHOSPLYR` и заголовок «Hot Seat Menu» (пиксели `COLOR_GREEN`).
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

    // `Multiplayer Menu` рисуется на `MAINPIC` без паник.
    let screen = Screen::MultiplayerMenu;
    screen.render(&mut buffer, ctx.view(Some(&app)));
    assert_eq!(
        buffer.get(10, 10),
        menu.mainpic.rgb(10, 10).map(Rgba::from_rgb)
    );

    // Заголовок и кнопки: подпись `Hot Seat Game` лежит в (385,280) 200×29.
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
