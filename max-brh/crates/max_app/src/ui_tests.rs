use super::*;

use std::cell::RefCell;
use std::rc::Rc;

use max_net::GameStatus;

use crate::AppConfig;

#[derive(Default)]
struct MockLog {
    list_calls: usize,
    create_calls: usize,
    last_params: Option<CreateGameParams>,
}

struct MockClient {
    log: Rc<RefCell<MockLog>>,
    games: Vec<GameInfo>,
    created: Option<GameInfo>,
    list_error: Option<String>,
    create_error: Option<String>,
}

impl MockClient {
    fn new(games: Vec<GameInfo>, created: Option<GameInfo>) -> (Self, Rc<RefCell<MockLog>>) {
        let log = Rc::new(RefCell::new(MockLog::default()));
        let client = Self {
            log: Rc::clone(&log),
            games,
            created,
            list_error: None,
            create_error: None,
        };

        (client, log)
    }
}

impl MenuClient for MockClient {
    fn list_games(&mut self) -> Result<Vec<GameInfo>, String> {
        self.log.borrow_mut().list_calls += 1;
        self.list_error
            .clone()
            .map_or_else(|| Ok(self.games.clone()), Err)
    }

    fn create_game(&mut self, params: CreateGameParams) -> Result<GameInfo, String> {
        self.log.borrow_mut().create_calls += 1;
        self.log.borrow_mut().last_params = Some(params);
        self.create_error.clone().map_or_else(
            || {
                self.created
                    .clone()
                    .ok_or_else(|| "mock: no created game".to_string())
            },
            Err,
        )
    }
}

fn game(id: &str, name: &str) -> GameInfo {
    GameInfo {
        id: id.to_string(),
        name: name.to_string(),
        world: 3,
        status: GameStatus::Running,
        port: 31554,
        players: 1,
        min_players: 2,
        password_protected: false,
    }
}

/// Backend that counts `play` calls (sound in tests without a device).
#[derive(Debug)]
struct CountingBackend {
    calls: std::sync::Arc<std::sync::Mutex<usize>>,
}

impl crate::audio::AudioBackend for CountingBackend {
    fn name(&self) -> &'static str {
            "counting"
    }

    fn play(
        &mut self,
        _clip: std::sync::Arc<max_assets::WavClip>,
        _mode: crate::audio::PlayMode,
    ) -> Result<(), String> {
        *self.calls.lock().expect("lock") += 1;

        Ok(())
    }
}

fn context(
    games: Vec<GameInfo>,
    created: Option<GameInfo>,
) -> (UiContext, Rc<RefCell<MockLog>>) {
    let (client, log) = MockClient::new(games, created);
    (UiContext::new(Box::new(client), 512, 384), log)
}

fn key(screen: &mut Screen, ctx: &mut UiContext, key: Key) -> Transition {
    screen.handle_input(ctx, InputEvent::Key(key))
}

fn char(screen: &mut Screen, ctx: &mut UiContext, c: char) -> Transition {
    key(screen, ctx, Key::Char(c))
}

#[test]
fn main_menu_opens_the_local_setup_and_escape_goes_back() {
    let (mut ctx, _log) = context(Vec::new(), None);
    let mut screen = Screen::MainMenu;

    // M-SP: «New Game» opens the `New Game` submenu (`menu.cpp:2171`).
    assert_eq!(key(&mut screen, &mut ctx, Key::Enter), Transition::None);
    assert_eq!(screen, Screen::NewGameMenu { tips: None });

    // `Custom Game` (`_(3d29)`) opens the local match setup (Options).
    let custom = newgame::new_game_rect(NewGameAction::Custom).expect("Custom Game");
    screen.handle_input(
        &mut ctx,
        InputEvent::MouseClick(
            custom.ulx + custom.width / 2,
            custom.uly + custom.height / 2,
        ),
    );
    assert!(matches!(screen, Screen::LocalSetup(_)));
    if let Screen::LocalSetup(form) = &screen {
        assert_eq!(form.screen, crate::setup::SetupScreen::Options);
    }

    // Enter leads to the next screen (Planet).
    key(&mut screen, &mut ctx, Key::Enter);
    if let Screen::LocalSetup(form) = &screen {
        assert_eq!(form.screen, crate::setup::SetupScreen::Planet);
    }

    // Escape returns to the `New Game` submenu (`menu_custom_game_menu` cancel
    // continues the `menu_new_game_menu_loop`), the second Escape to the main menu.
    key(&mut screen, &mut ctx, Key::Escape);
    assert_eq!(screen, Screen::NewGameMenu { tips: None });
    key(&mut screen, &mut ctx, Key::Escape);
    assert_eq!(screen, Screen::MainMenu);
}

#[test]
fn multiplayer_menu_opens_the_hot_seat_setup() {
    let (mut ctx, _log) = context(Vec::new(), None);
    let mut screen = Screen::MainMenu;

    // Main menu → «Multiplayer Game» (`menu.cpp:2361`).
    char(&mut screen, &mut ctx, 'j');
    assert_eq!(screen, Screen::MultiplayerMenu);

    // The `Hot Seat Game` button (385,280) 200×29.
    let button = menu::MULTIPLAYER_BUTTONS[2];
    let transition = screen.handle_input(
        &mut ctx,
        InputEvent::MouseClick(
            button.rect.ulx + button.rect.width / 2,
            button.rect.uly + button.rect.height / 2,
        ),
    );

    assert_eq!(transition, Transition::None);
    match &screen {
        Screen::LocalSetup(form) => {
            assert!(form.hot_seat);
            assert_eq!(form.players_title(), "Hot Seat Menu");
            assert_eq!(form.setup.human_count(), 2);
            assert_eq!(form.screen, crate::setup::SetupScreen::Options);
        }
        other => panic!("ожидалась настройка hot-seat, получено {other:?}"),
    }

    // Esc from the first screen returns to `Multiplayer Menu`, not to the main one.
    key(&mut screen, &mut ctx, Key::Escape);
    assert_eq!(screen, Screen::MultiplayerMenu);

    // Original hotkey `Shift+O` (`GNW_KB_KEY_SHIFT_O`).
    char(&mut screen, &mut ctx, 'o');
    assert!(matches!(screen, Screen::LocalSetup(_)));

    // `Cancel` (`Shift+C`) returns to the main menu.
    key(&mut screen, &mut ctx, Key::Escape);
    key(&mut screen, &mut ctx, Key::Escape);
    assert_eq!(screen, Screen::MainMenu);
}

#[test]
fn planet_click_changes_world_and_keeps_numeric_fallback() {
    let (mut ctx, _log) = context(Vec::new(), None);
    let mut screen = Screen::CreateForm(CreateForm::default());

    // A click on the 13th planet (index 12) selects it.
    let cell = planet_cell_rect(ctx.width, ctx.height, 12);
    let transition = screen.handle_input(
        &mut ctx,
        InputEvent::MouseClick(cell.x + cell.width / 2, cell.y + cell.height / 2),
    );

    assert_eq!(transition, Transition::None);
    if let Screen::CreateForm(form) = &screen {
        assert_eq!(form.world, 12);
        assert_eq!(form.focus, CreateField::World);
    } else {
        panic!("ожидалась форма создания");
    }

    // Numeric fallback: Tab to the World field and digits still work.
    if let Screen::CreateForm(form) = &mut screen {
        form.focus = CreateField::Name;
        form.world = 0;
    }
    key(&mut screen, &mut ctx, Key::Tab);
    char(&mut screen, &mut ctx, '7');

    if let Screen::CreateForm(form) = &screen {
        assert_eq!(form.world, 7, "цифровой ввод — fallback");
    }

    // A click on the planet name (bottom of the cell) also selects it.
    let cell = planet_cell_rect(ctx.width, ctx.height, 23);
    screen.handle_input(
        &mut ctx,
        InputEvent::MouseClick(cell.x + 2, cell.y + cell.height - 2),
    );
    if let Screen::CreateForm(form) = &screen {
        assert_eq!(form.world, 23);
    }
}

#[test]
fn planet_grid_is_eight_by_three_and_inside_the_form() {
    assert_eq!(PLANET_COLUMNS * PLANET_ROWS, PLANET_COUNT as i32);

    for (width, height) in [(512, 384), (640, 480), (320, 240)] {
        let grid = planet_grid_rect(width, height);
        assert!(grid.width > 0 && grid.height > 0);

        for index in 0..PLANET_COUNT {
            let cell = planet_cell_rect(width, height, index);
            assert!(cell.x >= grid.x && cell.x + cell.width <= grid.x + grid.width);
            assert!(cell.y >= grid.y && cell.y + cell.height <= grid.y + grid.height);
            assert_eq!(planet_cell_at(width, height, cell.x, cell.y), Some(index));
        }

        assert_eq!(planet_cell_at(width, height, 0, 0), None);
    }
}

#[test]
fn create_form_paints_planet_pictures_from_max_res() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let max_res = root.join("max-original/MAX.RES");
    let color_pal = root.join("max-original/COLOR.PAL");
    let patches = root.join("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        return;
    }

    let config = AppConfig {
        max_res,
        color_pal,
        patches_res: patches,
        ..Default::default()
    };
    let mut app = crate::App::new(config);
    app.init_assets().expect("ассеты игры загружаются");

    let form = CreateForm {
        world: 12,
        ..Default::default()
    };
    let mut buffer = RgbaBuffer::filled(640, 480, Rgba::rgb(1, 2, 3));
    let view = ScreenView {
        width: 640,
        height: 480,
        status: "planets",
        mouse: None,
        focus: 0,
        app: Some(&app),
        missions: None,
    };
    Screen::CreateForm(form).render(&mut buffer, view);

    let cell = planet_cell_rect(640, 480, 12);
    let image = Rect::new(cell.x + 3, cell.y + 3, cell.width - 6, cell.height - 16);
    let mut colors = std::collections::BTreeSet::new();

    for y in image.y..image.y + image.height {
        for x in image.x..image.x + image.width {
            if let Some(pixel) = buffer.get(x, y) {
                colors.insert([pixel.r, pixel.g, pixel.b]);
            }
        }
    }

    assert!(
        colors.len() > 8,
            "картинка планеты должна давать больше 8 цветов, а не заливку: {colors:?}"
    );
}

#[test]
fn create_form_creates_a_game_and_enters_the_session_with_mock_client() {
    let created = game("created-1", "Duel");
    let (mut ctx, log) = context(Vec::new(), Some(created.clone()));
    let mut screen = Screen::MainMenu;

    // The network form is available from `Multiplayer Menu`: «Multiplayer Game» →
    // `Join Network Game` (game list) → `c` (create a game).
    char(&mut screen, &mut ctx, 'j');
    char(&mut screen, &mut ctx, 'j');
    char(&mut screen, &mut ctx, 'c');
    for c in "Duel".chars() {
        char(&mut screen, &mut ctx, c);
    }

    // World 0 by default; check field switching and numeric input.
    key(&mut screen, &mut ctx, Key::Tab);
    char(&mut screen, &mut ctx, '1');
    char(&mut screen, &mut ctx, '2');
    if let Screen::CreateForm(form) = &screen {
        assert_eq!(form.world, 12);
    }

    let transition = key(&mut screen, &mut ctx, Key::Enter);

    assert_eq!(
        transition,
        Transition::StartSession(SessionRequest {
            game_id: "created-1".to_string(),
            password: None,
        })
    );
    assert_eq!(
        screen,
        Screen::InSession {
            request: SessionRequest {
                game_id: "created-1".to_string(),
                password: None,
            }
        }
    );

    let log = log.borrow();
    assert_eq!(log.create_calls, 1);
    assert_eq!(
        log.list_calls, 1,
            "Create не возвращается в список: единственный ListGames — вход в список"
    );
    assert!(
        ctx.status.contains("Created \"Duel\""),
            "статус подтверждает создание: {}",
        ctx.status
    );
    let params = log.last_params.as_ref().expect("параметры CreateGame");
    assert_eq!(params.name.as_deref(), Some("Duel"));
    assert_eq!(params.world, Some(12));
    assert_eq!(params.min_players, Some(2));
    assert_eq!(params.password, None);
}

#[test]
fn create_failure_keeps_the_form_and_reports_status() {
    let (client, _log) = MockClient::new(Vec::new(), None);
    let client = MockClient {
        create_error: Some("horizon down".to_string()),
        ..client
    };
    let mut ctx = UiContext::new(Box::new(client), 512, 384);
    let mut screen = Screen::CreateForm(CreateForm::default());

    let transition = key(&mut screen, &mut ctx, Key::Enter);

    assert_eq!(transition, Transition::None);
    assert!(matches!(screen, Screen::CreateForm(_)));
    assert!(ctx.status.contains("horizon down"));
}

#[test]
fn join_list_lists_games_and_selects_with_keyboard() {
    let (mut ctx, log) = context(vec![game("g1", "Alpha"), game("g2", "Beta")], None);
    let mut screen = Screen::MainMenu;

    // MainMenu: «Multiplayer Game» opens `Multiplayer Menu` (`menu.cpp:2361`).
    key(&mut screen, &mut ctx, Key::Down);
    key(&mut screen, &mut ctx, Key::Enter);
    assert_eq!(screen, Screen::MultiplayerMenu);

    // `Join Network Game` — the Horizon game list.
    char(&mut screen, &mut ctx, 'j');

    assert_eq!(
        screen,
        Screen::JoinList {
            games: vec![game("g1", "Alpha"), game("g2", "Beta")],
            selected: 0,
        }
    );
    assert_eq!(log.borrow().list_calls, 1);

    key(&mut screen, &mut ctx, Key::Down);
    if let Screen::JoinList { selected, .. } = &screen {
        assert_eq!(*selected, 1);
    } else {
        panic!("ожидался список игр");
    }

    let transition = key(&mut screen, &mut ctx, Key::Enter);
    assert_eq!(
        transition,
        Transition::StartSession(SessionRequest {
            game_id: "g2".to_string(),
            password: None,
        })
    );
    assert!(matches!(screen, Screen::InSession { .. }));
}

#[test]
fn join_list_mouse_click_selects_a_row() {
    let games = vec![game("g1", "Alpha"), game("g2", "Beta")];
    let (mut ctx, _log) = context(games.clone(), None);
    let mut screen = Screen::JoinList { games, selected: 0 };

    let ui = build_join_list(
        ctx.width,
        ctx.height,
        &ctx.status,
        None,
        match &screen {
            Screen::JoinList { games, .. } => games,
            _ => unreachable!(),
        },
        0,
    );
    let list = ui.list_box(JOIN_LIST).expect("ListBox");
    let point = Point::new(list.rect.x + 4, list.rect.y + list.row_height + 2);

    screen.handle_input(&mut ctx, InputEvent::MouseClick(point.x, point.y));

    if let Screen::JoinList { selected, .. } = &screen {
        assert_eq!(*selected, 1);
    } else {
        panic!("ожидался список игр");
    }
}

#[test]
fn join_list_refresh_uses_list_games_again() {
    let (mut ctx, log) = context(vec![game("g1", "Alpha")], None);
    let mut screen = Screen::JoinList {
        games: vec![game("g1", "Alpha")],
        selected: 0,
    };

    key(&mut screen, &mut ctx, Key::Char('r'));

    assert_eq!(log.borrow().list_calls, 1);
    assert!(matches!(screen, Screen::JoinList { .. }));
}

#[test]
fn list_games_failure_stays_in_the_menu() {
    let (client, _log) = MockClient::new(Vec::new(), None);
    let client = MockClient {
        list_error: Some("no horizon".to_string()),
        ..client
    };
    let mut ctx = UiContext::new(Box::new(client), 512, 384);
    let mut screen = Screen::MainMenu;

    key(&mut screen, &mut ctx, Key::Down);
    key(&mut screen, &mut ctx, Key::Enter);
    char(&mut screen, &mut ctx, 'j');

    // A network error does not leave the menu: we stay in `Multiplayer Menu` with
    // the status line (the game list is also available by a repeated `j`).
    assert_eq!(screen, Screen::MultiplayerMenu);
    assert!(ctx.status.contains("no horizon"));
}

/// Item E: in a 640×480 frame the menu is drawn from `MAX.RES` resources, not
/// procedurally.
#[test]
fn native_frame_menus_are_drawn_from_max_res() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let max_res = root.join("max-original/MAX.RES");
    let color_pal = root.join("max-original/COLOR.PAL");
    let patches = root.join("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        return;
    }

    let mut app = crate::App::new(AppConfig {
        max_res,
        color_pal,
        patches_res: patches,
        ..Default::default()
    });
    app.init_assets().expect("ассеты игры");
    let menu = app
        .assets()
        .and_then(|assets| assets.menu.as_ref())
        .expect("MenuAssets из MAX.RES");

    let view = ScreenView {
        width: 640,
        height: 480,
        status: "status",
        mouse: Some(Point::new(390, 180)),
        focus: 0,
        app: Some(&app),
        missions: None,
    };

    // Main menu: background — MAINPIC.
    let mut buffer = RgbaBuffer::new(640, 480);
    Screen::MainMenu.render(&mut buffer, view);
    assert_eq!(
        buffer.get(0, 0),
        menu.mainpic.rgb(0, 0).map(Rgba::from_rgb),
            "фон главного меню — MAINPIC"
    );

    // Creation form: background — PLANETSE, MNUBTN buttons at the bottom.
    let mut buffer = RgbaBuffer::new(640, 480);
    Screen::CreateForm(CreateForm::default()).render(&mut buffer, view);
    assert_eq!(
        buffer.get(600, 300),
        menu.planet_background().rgb(600, 300).map(Rgba::from_rgb),
            "фон формы — PLANETSE"
    );

    // Game list: background — MULTGAME.
    let mut buffer = RgbaBuffer::new(640, 480);
    Screen::JoinList {
        games: vec![game("g1", "Alpha")],
        selected: 0,
    }
    .render(&mut buffer, view);
    assert_eq!(
        buffer.get(10, 10),
        menu.join_background().rgb(10, 10).map(Rgba::from_rgb),
            "фон списка игр — MULTGAME"
    );
}

/// Item E: clicks on the main menu buttons hit the original
/// `menu.cpp` rectangles (not the procedural layout).
#[test]
fn main_menu_clicks_use_the_cpp_button_rectangles() {
    let (mut ctx, _log) = context(Vec::new(), None);
    ctx.width = 640;
    ctx.height = 480;
    let mut screen = Screen::MainMenu;

    let create = menu::main_menu_rect(menu::MainMenuAction::Create).expect("New Game");
    let transition = screen.handle_input(
        &mut ctx,
        InputEvent::MouseClick(
            create.ulx + create.width / 2,
            create.uly + create.height / 2,
        ),
    );

    assert_eq!(transition, Transition::None);
    assert_eq!(
        screen,
        Screen::NewGameMenu { tips: None },
            "New Game открывает подменю New Game Menu"
    );

    // The `Custom Game` button of the submenu opens the local match setup.
    let custom = newgame::new_game_rect(NewGameAction::Custom).expect("Custom Game");
    screen.handle_input(
        &mut ctx,
        InputEvent::MouseClick(custom.ulx + 4, custom.uly + 4),
    );
    assert!(
        matches!(screen, Screen::LocalSetup(_)),
            "Custom Game открывает настройку локальной партии"
    );

    let mut screen = Screen::MainMenu;
    let exit = menu::main_menu_rect(menu::MainMenuAction::Quit).expect("Exit");
    assert_eq!(
        screen.handle_input(&mut ctx, InputEvent::MouseClick(exit.ulx + 4, exit.uly + 4),),
        Transition::Quit
    );
}

/// Item F: a click in the 1280×960 window (scale 2) is converted to the 640×480 frame and
/// hits the same map cell as a click in the frame.
#[test]
fn window_click_at_2x_lands_in_the_same_map_cell() {
    use max_render::frame_scale::FrameScale;
    use max_render::layout;

    let scale = FrameScale::native(1280, 960);
    assert_eq!(scale.scale(), 2);

    let viewport = panels::map_viewport(&Viewport::new(640, 480));
    // A point in the map area: (180,18) + 3.5 cells by x and 2.5 by y.
    let frame_click = (
        layout::MAIN_MAP.ulx + 3 * 64 + 32,
        layout::MAIN_MAP.uly + 2 * 64 + 32,
    );
    let local = (
        frame_click.0 - layout::MAIN_MAP.ulx,
        frame_click.1 - layout::MAIN_MAP.uly,
    );
    let expected_grid = viewport.screen_to_grid(local.0, local.1);

    let window_click = scale.frame_to_window(frame_click.0, frame_click.1);
    let mapped = scale
        .window_to_frame(window_click.0 + 1, window_click.1 + 1)
        .expect("клик внутри кадра");
    let (map_x, map_y) = panels::screen_to_map(mapped.0, mapped.1).expect("точка на карте");
    let grid = viewport.screen_to_grid(map_x, map_y);

    assert_eq!(grid, expected_grid);
    assert_eq!(grid, (56, 55), "клетка под точкой (180+224, 18+160)");

    // A click on the black field of the 1000×700 window (letterbox) does not reach the frame.
    let letterboxed = FrameScale::native(1000, 700);
    assert_eq!(letterboxed.window_to_frame(10, 10), None);
}

#[test]
fn setup_help_button_opens_and_closes_the_help_window() {
    let (mut ctx, _log) = context(Vec::new(), None);
    let mut screen = Screen::MainMenu;

    key(&mut screen, &mut ctx, Key::Enter);
    assert_eq!(screen, Screen::NewGameMenu { tips: None });

    // `Custom Game` opens the local match setup (Options).
    let custom = newgame::new_game_rect(NewGameAction::Custom).expect("Custom Game");
    screen.handle_input(
        &mut ctx,
        InputEvent::MouseClick(custom.ulx + 4, custom.uly + 4),
    );
    assert!(matches!(screen, Screen::LocalSetup(_)));

    // The `?` button (465,438 48×40) on the Options screen.
    let transition = screen.handle_input(&mut ctx, InputEvent::MouseClick(489, 458));
    assert_eq!(transition, Transition::None);

    match &screen {
        Screen::Help {
            screen, help, mode, ..
        } => {
            // C++ `HelpMenu_Menu`: the `?` button enables help mode, the window
            // does not exist yet; an entry is chosen by clicking an element.
            assert!(help.is_none(), "режим помощи: окна ещё нет");
            assert_eq!(mode.section, HelpSection::Options);
            assert_eq!(mode.placement, HelpPlacement::Menu);
            assert!(matches!(
                screen.as_ref(),
                Screen::LocalSetup(form) if form.screen == crate::setup::SetupScreen::Options
            ));
        }
        other => panic!("ожидалось окно помощи, получено {other:?}"),
    }

    // A click on the `PLAY_MODE` element (426..632, 39..227) opens the entry.
    screen.handle_input(&mut ctx, InputEvent::MouseClick(529, 133));
    if let Screen::Help {
        help: Some(help), ..
    } = &screen
    {
        assert_eq!(help.section(), HelpSection::Options);
        assert_eq!(help.entry_name(), Some("PLAY_MODE"));
    } else {
        panic!("клик по элементу не открыл запись");
    }

    // `PageDown` scrolls the entry; `Esc` closes the window and returns to the
    // mode (C++ `HelpMenu::Run` returns to `HelpMenu_Menu`).
    let rows_before = match &screen {
        Screen::Help {
            help: Some(help), ..
        } => help.line_count(),
        _ => 0,
    };
    screen.handle_input(&mut ctx, InputEvent::Key(Key::PageDown));
    if let Screen::Help {
        help: Some(help), ..
    } = &screen
    {
        if rows_before > crate::helpmenu::ROWS_PER_PAGE {
            assert_eq!(help.row_index(), crate::helpmenu::ROWS_PER_PAGE);
        } else {
            assert_eq!(help.row_index(), 0);
        }
    }

    screen.handle_input(&mut ctx, InputEvent::Key(Key::Escape));
    assert!(
        matches!(&screen, Screen::Help { help: None, .. }),
            "Esc закрывает окно, режим остаётся"
    );

    // A second `Esc` disables the mode and returns the Options screen.
    screen.handle_input(&mut ctx, InputEvent::Key(Key::Escape));
    assert!(matches!(
        screen,
        Screen::LocalSetup(form) if form.screen == crate::setup::SetupScreen::Options
    ));
}

#[test]
fn research_menu_opens_for_a_powered_center_and_commits() {
    use max_core::enums::{ResearchTopic, UnitOrder, UnitType};
    use max_core::geo::Point;
    use max_core::team::TeamId;

    let (_ctx, _log) = context(Vec::new(), None);
    let mut app = App::new(AppConfig::default());
    let id = app
        .universe
        .spawn_unit(UnitType::ResearchCentre, TeamId::RED, Point::new(20, 20), 0)
        .expect("center");
    let mut unit = app.state_ref().unit(id).cloned().expect("unit");
    unit.orders = UnitOrder::PowerOn;
    unit.research_topic = ResearchTopic::Attack;
    app.universe.write_unit(id, unit);
    app.universe
        .update_research_progress(TeamId::RED, ResearchTopic::Attack, 1);
    app.hud_mut().select(Some(id));

    assert!(app.open_research_menu(), "меню открывается для центра");
    assert_eq!(app.research_menu().map(|menu| menu.active_centers), Some(1));

    // The `R` key does not open the menu again (already open).
    assert_eq!(
        handle_session_input(&mut app, InputEvent::Key(Key::Char('r'))),
        Transition::None
    );

    // `←`/`→` and `Done` through the same input path as the player.
    assert_eq!(
        handle_session_input(&mut app, InputEvent::Key(Key::Down)),
        Transition::None
    );
    handle_session_input(&mut app, InputEvent::Key(Key::Left));
    handle_session_input(&mut app, InputEvent::Key(Key::Right));
    assert_eq!(
        handle_session_input(&mut app, InputEvent::Key(Key::Enter)),
        Transition::None
    );
    assert!(app.research_menu().is_none());
    assert_eq!(
        app.state_ref().research_allocation(TeamId::RED)[ResearchTopic::Attack as usize],
        1
    );
}

#[test]
fn panel_help_opens_game_screen_help_and_escape_closes_it() {
    use max_core::enums::{UnitOrder, UnitType};
    use max_core::geo::Point;
    use max_core::team::TeamId;

    let mut app = App::new(AppConfig::default());
    let id = app
        .universe
        .spawn_unit(UnitType::Surveyor, TeamId::RED, Point::new(20, 20), 0)
        .expect("unit");
    let mut unit = app.state_ref().unit(id).cloned().expect("unit");
    unit.orders = UnitOrder::Await;
    app.universe.write_unit(id, unit);

    // `?`/the HELP_OF button enables help mode (`HelpMenu_Menu`): there is no window,
    // an entry is chosen by clicking an element.
    assert!(app.open_help(HelpSection::GameScreen));
    assert_eq!(
        app.help_mode().map(|mode| mode.section),
        Some(HelpSection::GameScreen)
    );
    assert!(app.help_screen().is_none());

    // A click on the `FILES` panel — the `FILES_BUTTON` entry (17..83, 4..20).
    assert_eq!(
        handle_session_input(&mut app, InputEvent::MouseClick(50, 12)),
        Transition::None
    );
    assert_eq!(
        app.help_screen().and_then(|help| help.entry_name()),
        Some("FILES_BUTTON")
    );

    // `Esc` closes the window and returns to the mode (C++ `HelpMenu::Run`).
    handle_session_input(&mut app, InputEvent::Key(Key::Escape));
    assert!(app.help_screen().is_none());
    assert!(app.help_mode().is_some());

    // A second `Esc` disables the mode.
    handle_session_input(&mut app, InputEvent::Key(Key::Escape));
    assert!(app.help_mode().is_none());
}

#[test]
fn all_screens_render_without_panics() {
    let app = crate::App::new(AppConfig::default().with_screen(320, 240));
    let mut buffer = RgbaBuffer::new(320, 240);

    let screens = [
        Screen::MainMenu,
        Screen::JoinList {
            games: vec![game("g1", "Alpha")],
            selected: 0,
        },
        Screen::CreateForm(CreateForm::default()),
        Screen::MultiplayerMenu,
        Screen::Help {
            screen: Box::new(Screen::LocalSetup(GameSetupForm::default())),
            help: Some(helpmenu::HelpScreen::new(
                HelpSection::Options,
                HelpPlacement::Menu,
            )),
            mode: helpmenu::HelpMode::new(HelpSection::Options, HelpPlacement::Menu),
        },
        Screen::InSession {
            request: SessionRequest {
                game_id: "g1".to_string(),
                password: None,
            },
        },
    ];

    for screen in &screens {
        let view = ScreenView {
            width: 320,
            height: 240,
            status: "status",
            mouse: Some(Point::new(10, 10)),
            focus: 0,
            app: Some(&app),
            missions: None,
        };
        screen.render(&mut buffer, view);

        assert_eq!(buffer.get(0, 0).map(|pixel| pixel.a), Some(255));
        assert!(
            buffer.pixels().chunks_exact(4).any(|pixel| pixel[3] != 0),
                "экран {screen:?} не нарисовал ни одного пикселя"
        );
    }

    // Without App (headless rendering before the network) the session still draws the status.
    let mut ctx2 = UiContext::new(Box::new(MockClient::new(Vec::new(), None).0), 64, 32);
    ctx2.status = "offline".to_string();
    let screen = Screen::InSession {
        request: SessionRequest {
            game_id: "g1".to_string(),
            password: None,
        },
    };
    let mut small = RgbaBuffer::new(64, 32);
    screen.render(&mut small, ctx2.view(None));
    assert!(small.pixels().chunks_exact(4).all(|pixel| pixel[3] == 255));
}

#[test]
fn create_params_carry_password_and_opponents() {
    let form = CreateForm {
        name: "  Password game  ".to_string(),
        world: 7,
        timer: 90,
        endturn: 15,
        opponent: 3,
        password: "sesame".to_string(),
        focus: CreateField::Name,
    };

    let params = form.create_params();
    assert_eq!(params.name.as_deref(), Some("Password game"));
    assert_eq!(params.world, Some(7));
    assert_eq!(params.min_players, Some(4));
    assert_eq!(params.password.as_deref(), Some("sesame"));
    assert_eq!(form.password_value().as_deref(), Some("sesame"));
}

/// M6.2: a click on a menu button calls `AudioPlayer::play_once(KBUY0)`.
///
/// The clip is put into the cache (`preload`), so the test needs neither game
/// data nor an audio device — exactly the UI hook is checked.
#[test]
fn menu_button_click_triggers_menu_sound() {
    let calls = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let (mut ctx, _log) = context(Vec::new(), None);
    ctx.audio = AudioPlayer::with_backend(Box::new(CountingBackend {
        calls: std::sync::Arc::clone(&calls),
    }));
    ctx.audio.preload(
        MENU_CLICK,
        max_assets::WavClip {
            sample_rate: 22050,
            channels: 1,
            samples: vec![1, 2, 3],
        },
    );

    let ui = build_main_menu(ctx.width, ctx.height, &ctx.status, ctx.mouse, ctx.focus);
    let create = ui.widgets[MAIN_CREATE].rect().center();
    let mut screen = Screen::MainMenu;

    let transition = screen.handle_input(&mut ctx, InputEvent::MouseClick(create.x, create.y));

    assert_eq!(transition, Transition::None);
    assert_eq!(
        screen,
        Screen::NewGameMenu { tips: None },
            "клик по New Game открывает подменю"
    );
    assert_eq!(*calls.lock().expect("lock"), 1, "клик должен озвучиваться");
    assert_eq!(ctx.audio.plays(), 1);
}
