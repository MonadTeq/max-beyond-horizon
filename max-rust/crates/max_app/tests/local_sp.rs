//! M-SP: сквозные тесты одиночной партии против ИИ.
//!
//! Проверяются:
//!
//! 1. настройка из UI (`GameSetupForm` → `LocalSetup.team_types`, планета,
//!    таймер) и переход `StartLocal`;
//! 2. локальный цикл: 5 ходов против ИИ — ходы чередуются, ИИ ходит сам
//!    (`Ai_BeginTurn` + задачи), `turn_counter` растёт;
//! 3. победа при уничтожении ИИ — `evaluate_status`/`finish_turn` дают исход,
//!    экран победы рисуется без паник;
//! 4. сохранение локальной партии (контейнер `MXLS`) после хода — round-trip;
//! 5. загрузка V70-сейва в локальную партию — состояние играется локальным
//!    циклом.

use std::path::{Path, PathBuf};

use max_app::local::{LocalPhase, LocalSave, LocalSetup};
use max_app::setup::{GameSetupForm, SetupScreen, SetupTransition};
use max_app::ui::{InputEvent, Key, MenuClient, Screen, Transition, UiContext};
use max_app::{App, AppConfig};
use max_core::enums::{PlayMode, VictoryType};
use max_core::geo::Point;
use max_core::team::TeamId;
use max_core::team::TeamKind;
use max_net::{CreateGameParams, GameInfo};
use max_render::buffer::PixelRect;

const V70_SAVE: &[u8] = include_bytes!("data/save1_v70.cam");

/// Клиент лобби-заглушки: настройке M-SP сеть не нужна.
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

/// Приложение с настоящими ассетами (иначе карту не сгенерировать).
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

fn context() -> UiContext {
    UiContext::new(Box::new(NoNetwork), 640, 480)
}

/// Турбо-прогон до хода игрока: не больше `limit` кадров.
fn run_until_player_turn(app: &mut App, limit: usize) -> bool {
    for _ in 0..limit {
        if app.local().map(|local| local.phase) == Some(LocalPhase::PlayerTurn) {
            return true;
        }
        if app.local().is_none_or(|local| local.finished()) {
            return false;
        }
        app.local_update(0.0);
    }

    app.local().map(|local| local.phase) == Some(LocalPhase::PlayerTurn)
}

fn run_until_finished(app: &mut App, limit: usize) -> bool {
    for _ in 0..limit {
        if app.local().is_some_and(|local| local.finished()) {
            return true;
        }
        if app.local().is_none() {
            return false;
        }
        app.local_update(0.0);
    }

    app.local().is_some_and(|local| local.finished())
}

/// Настройка: 1 человек (RED) + 1 компьютер (GREEN), остальные — пусто.
fn setup_vs_one_ai() -> LocalSetup {
    LocalSetup {
        player_name: "Tester".to_string(),
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
        victory_type: VictoryType::Duration,
        victory_limit: 50,
        team_types: [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ],
        seed: 7,
    }
}

// ---------------------------------------------------------------------------
// 1. Настройка из UI
// ---------------------------------------------------------------------------

#[test]
fn setup_form_produces_team_types_planet_and_timer() {
    let mut form = GameSetupForm::default();
    form.setup.player_name = "Alice".to_string();

    // Options: таймер 180 (строка "180"), конец хода 45.
    let timer_row = max_app::setup::CONFIG_ITEMS[18].rect;
    let transition = form.handle_input(
        InputEvent::MouseClick(timer_row.ulx + 2, timer_row.uly + 2),
        640,
        480,
    );
    assert_eq!(transition, SetupTransition::None);
    assert_eq!(form.setup.timer, 180);

    let endturn_row = max_app::setup::CONFIG_ITEMS[25].rect;
    form.handle_input(
        InputEvent::MouseClick(endturn_row.ulx + 2, endturn_row.uly + 2),
        640,
        480,
    );
    assert_eq!(form.setup.endturn, 45);

    // Done → Planet.
    let done = PixelRect::new(514, 438, 109, 40);
    form.handle_input(InputEvent::MouseClick(done.ulx + 2, done.uly + 2), 640, 480);
    assert_eq!(form.screen, SetupScreen::Planet);

    // Планета 8 (slot 2 группы 6).
    let slot = max_app::setup::PLANET_SLOTS[2];
    form.handle_input(InputEvent::MouseClick(slot.ulx + 4, slot.uly + 4), 640, 480);
    assert_eq!(form.setup.world, 2);

    // Done → Players.
    form.handle_input(InputEvent::MouseClick(done.ulx + 2, done.uly + 2), 640, 480);
    assert_eq!(form.screen, SetupScreen::Players);

    // GREEN (row 1) — Computer, BLUE (row 2) — None.
    let computer = max_app::setup::player_type_button_rect(1, 1);
    form.handle_input(
        InputEvent::MouseClick(computer.ulx + 4, computer.uly + 4),
        640,
        480,
    );
    let none = max_app::setup::player_type_button_rect(2, 2);
    form.handle_input(InputEvent::MouseClick(none.ulx + 4, none.uly + 4), 640, 480);

    assert_eq!(
        form.setup.team_types,
        [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None
        ]
    );
    assert_eq!(form.setup.human_team(), Some(TeamId::RED));

    // Done на экране команд → Start с этими настройками.
    let transition =
        form.handle_input(InputEvent::MouseClick(done.ulx + 2, done.uly + 2), 640, 480);

    match transition {
        SetupTransition::Start(setup) => {
            assert_eq!(setup.team_types[1], TeamKind::Computer);
            assert_eq!(setup.world, 2);
            assert_eq!(setup.timer, 180);
        }
        other => panic!("ожидался Start, получено {other:?}"),
    }
}

#[test]
fn screen_routes_setup_to_start_local_and_back() {
    let mut ctx = context();

    // MainMenu → LocalSetup (New Game).
    let mut screen = Screen::MainMenu;
    assert_eq!(
        screen.handle_input(&mut ctx, InputEvent::Key(Key::Enter)),
        Transition::None
    );
    assert!(matches!(screen, Screen::LocalSetup(_)));

    // Esc с первого экрана — назад в меню.
    assert_eq!(
        screen.handle_input(&mut ctx, InputEvent::Key(Key::Escape)),
        Transition::None
    );
    assert_eq!(screen, Screen::MainMenu);

    // Экран команд с готовыми настройками отдаёт StartLocal.
    let mut form = GameSetupForm::default();
    form.screen = SetupScreen::Players;
    form.setup.team_types = [
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::None,
        TeamKind::None,
    ];
    let mut screen = Screen::LocalSetup(form);
    let done = max_app::setup::PLAYER_BUTTONS[2].0;
    let transition =
        screen.handle_input(&mut ctx, InputEvent::MouseClick(done.ulx + 2, done.uly + 2));

    match transition {
        Transition::StartLocal(setup) => {
            assert_eq!(setup.team_types[0], TeamKind::Player);
            assert_eq!(setup.team_types[1], TeamKind::Computer);
        }
        other => panic!("ожидался StartLocal, получено {other:?}"),
    }
}

#[test]
fn players_screen_rejects_a_game_without_opponent() {
    let mut form = GameSetupForm::default();
    form.screen = SetupScreen::Players;
    form.setup.team_types = [
        TeamKind::Player,
        TeamKind::None,
        TeamKind::None,
        TeamKind::None,
    ];

    let done = max_app::setup::PLAYER_BUTTONS[2].0;
    let transition =
        form.handle_input(InputEvent::MouseClick(done.ulx + 2, done.uly + 2), 640, 480);

    assert_eq!(transition, SetupTransition::None);
    assert!(
        form.status.contains("two players"),
        "статус: {}",
        form.status
    );
}

// ---------------------------------------------------------------------------
// 2. Локальный цикл: 5 ходов против ИИ
// ---------------------------------------------------------------------------

#[test]
fn five_turns_against_the_ai_alternate_and_the_ai_acts() {
    let Some(mut app) = local_app() else {
        return;
    };

    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия стартует");

    assert!(app.local_active());
    assert_eq!(
        app.local().map(|local| local.phase),
        Some(LocalPhase::PlayerTurn)
    );
    assert_eq!(app.state().player_team, TeamId::RED);
    assert_eq!(app.state().turn_counter, 1);
    assert_eq!(app.state().teams[0].team_type, TeamKind::Player);
    assert_eq!(app.state().teams[1].team_type, TeamKind::Computer);

    let ai_units_before: Vec<_> = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .filter(|id| {
            app.state()
                .unit(*id)
                .is_some_and(|unit| unit.team == TeamId::GREEN)
        })
        .collect();
    assert!(!ai_units_before.is_empty(), "у ИИ есть стартовые юниты");

    let mut saw_ai_turn = false;

    for turn in 1..=5 {
        assert_eq!(
            app.local().map(|local| local.phase),
            Some(LocalPhase::PlayerTurn),
            "ход {turn}: должен ждать игрока"
        );
        assert_eq!(app.state().active_turn_team, TeamId::RED);
        assert_eq!(app.state().turn_counter, turn);

        app.local_end_turn();

        // Ход ИИ: фаза AiTurn и возврат к игроку.
        if app.local().map(|local| local.phase) == Some(LocalPhase::AiTurn(TeamId::GREEN)) {
            saw_ai_turn = true;
        }

        assert!(
            run_until_player_turn(&mut app, 20_000),
            "ход {turn}: ИИ должен завершить ход"
        );
    }

    assert!(saw_ai_turn, "ИИ должен ходить сам (AiTurn)");
    assert_eq!(app.state().turn_counter, 6, "5 ходов отыграно");

    // ИИ работал: у него есть задачи, а его юниты получали приказы.
    assert!(
        app.state().ai.task_manager.task_count() > 0,
        "у ИИ должны быть задачи (TaskManager)"
    );

    let ai_ordered = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state().unit(id))
        .filter(|unit| unit.team == TeamId::GREEN)
        .any(|unit| {
            !matches!(
                unit.orders,
                max_core::enums::UnitOrder::Await | max_core::enums::UnitOrder::Idle
            )
        });

    let ai_acted = ai_ordered
        || ai_units_before
            .iter()
            .any(|id| app.state().unit(*id).is_none())
        || app
            .state()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| app.state().unit(id))
            .filter(|unit| unit.team == TeamId::GREEN)
            .count()
            != ai_units_before.len();

    assert!(ai_acted, "ИИ должен отдавать приказы/двигаться");
}

#[test]
fn simultaneous_moves_mode_also_plays_turns() {
    let Some(mut app) = local_app() else {
        return;
    };

    let mut setup = setup_vs_one_ai();
    setup.play_mode = PlayMode::SimultaneousMoves;
    app.start_local_game(&setup).expect("локальная партия");

    for turn in 1..=2 {
        assert_eq!(app.state().turn_counter, turn);
        app.local_end_turn();
        assert!(
            run_until_player_turn(&mut app, 20_000),
            "ход {turn}: возврат к игроку"
        );
    }
}

#[test]
fn turn_timer_expires_and_closes_the_turn() {
    let Some(mut app) = local_app() else {
        return;
    };

    let mut setup = setup_vs_one_ai();
    setup.timer = 2;
    app.start_local_game(&setup).expect("локальная партия");

    let before = app.state().turn_counter;

    // 3 секунды кадрами по 1 с: таймер 2 с истекает и закрывает ход.
    app.local_update(1.0);
    app.local_update(1.0);
    app.local_update(1.0);

    assert!(run_until_player_turn(&mut app, 20_000));
    assert!(
        app.state().turn_counter > before,
        "таймер хода должен закрыть ход"
    );
}

#[test]
fn pause_stops_ticks_and_timer() {
    let Some(mut app) = local_app() else {
        return;
    };

    let mut setup = setup_vs_one_ai();
    setup.timer = 2;
    app.start_local_game(&setup).expect("локальная партия");

    let ticks_before = app.ticks();
    assert!(app.local_toggle_pause());
    let update = app.local_update(5.0);
    assert!(!update.ticked, "на паузе тиков нет");
    assert_eq!(app.ticks(), ticks_before);
    assert_eq!(
        app.local().map(|local| local.phase),
        Some(LocalPhase::PlayerTurn),
        "пауза не закрывает ход"
    );

    assert!(!app.local_toggle_pause());
}

#[test]
fn human_on_a_later_slot_starts_with_the_ai_turns() {
    let Some(mut app) = local_app() else {
        return;
    };

    // Человек — GREEN (слот 1), RED — компьютер: раунд начинается с хода ИИ.
    let mut setup = setup_vs_one_ai();
    setup.team_types = [
        TeamKind::Computer,
        TeamKind::Player,
        TeamKind::None,
        TeamKind::None,
    ];

    app.start_local_game(&setup).expect("локальная партия");

    assert_eq!(app.state().player_team, TeamId::GREEN);
    assert!(
        matches!(
            app.local().map(|local| local.phase),
            Some(LocalPhase::AiTurn(TeamId::RED))
        ),
        "первым ходит компьютер RED"
    );

    // Доходим до хода человека: RED отыграл, дальше GREEN.
    assert!(run_until_player_turn(&mut app, 40_000));
    assert_eq!(
        app.local().map(|local| local.phase),
        Some(LocalPhase::PlayerTurn)
    );
    assert_eq!(app.state().active_turn_team, TeamId::GREEN);

    app.local_end_turn();
    assert!(
        run_until_player_turn(&mut app, 40_000),
        "следующий раунд снова доходит до игрока"
    );
    assert!(app.state().turn_counter >= 2);
}

// ---------------------------------------------------------------------------
// 3. Победа при элиминации ИИ
// ---------------------------------------------------------------------------

#[test]
fn eliminating_the_ai_wins_and_the_game_over_screen_renders() {
    let Some(mut app) = local_app() else {
        return;
    };

    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия");

    // Уничтожаем все юниты ИИ — как будто его выбили в бою.
    let ai_units: Vec<_> = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .filter(|id| {
            app.state()
                .unit(*id)
                .is_some_and(|unit| unit.team == TeamId::GREEN)
        })
        .collect();
    assert!(!ai_units.is_empty());

    for id in ai_units {
        app.state_mut().destroy_unit(id);
    }

    // Закрываем раунд: ход игрока → ход ИИ (без юнитов) → finish_turn.
    app.local_end_turn();
    assert!(
        run_until_finished(&mut app, 40_000),
        "партия должна завершиться после элиминации ИИ"
    );

    let local = app.local().expect("локальная партия");
    let outcome = local.outcome.as_ref().expect("исход партии");

    assert!(outcome.victory, "игрок должен победить");
    assert_eq!(
        outcome.status.team_rank[TeamId::RED.0 as usize],
        1,
        "место победителя"
    );
    assert!(outcome.turn_counter >= 1);

    // Экран победы рисуется без паник и рисует текст/картинки.
    let mut buffer = max_render::buffer::RgbaBuffer::new(640, 480);
    let mut ctx = context();
    let screen = Screen::GameOver;
    screen.render(&mut buffer, ctx.view(Some(&app)));

    let painted = (0..640)
        .flat_map(|x| (0..480).map(move |y| (x, y)))
        .filter(|(x, y)| {
            buffer
                .get(*x, *y)
                .is_some_and(|color| color != max_render::buffer::Rgba::rgb(0, 0, 0))
        })
        .count();
    assert!(painted > 100_000, "фон экрана победы закрашен: {painted}");

    // Серебряный текст (`Fonts_BrightSilverColor`) присутствует.
    let palette = app
        .assets()
        .and_then(|assets| assets.menu.as_ref())
        .map(|menu| menu.palette)
        .expect("menu assets");
    let silver = max_app::winloss::silver_color(&palette);
    let silver_pixels = (0..640)
        .flat_map(|x| (0..480).map(move |y| (x, y)))
        .filter(|(x, y)| buffer.get(*x, *y) == Some(silver))
        .count();
    assert!(silver_pixels > 50, "текст экрана: {silver_pixels} пикселей");

    // Клик по OK — выход из экрана.
    let ok = max_app::winloss::OK_RECT;
    let mut screen = Screen::GameOver;
    let transition = screen.handle_input(&mut ctx, InputEvent::MouseClick(ok.ulx + 4, ok.uly + 4));
    assert_eq!(transition, Transition::LeaveSession);
}

// ---------------------------------------------------------------------------
// 4. Сохранение локальной партии после хода
// ---------------------------------------------------------------------------

#[test]
fn local_save_roundtrips_after_a_turn() {
    let Some(mut app) = local_app() else {
        return;
    };

    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия");

    app.local_end_turn();
    assert!(run_until_player_turn(&mut app, 20_000));

    let bytes = app.local_save_bytes().expect("сохранение локальной партии");
    let save = LocalSave::decode(&bytes).expect("контейнер разбирается");

    assert_eq!(save.setup, setup_vs_one_ai());
    assert_eq!(save.phase, LocalPhase::PlayerTurn);
    assert_eq!(save.turn_counter, app.state().turn_counter);
    assert!(!save.finished);
    assert_eq!(save.snapshot, app.state().snapshot_bytes());
    assert_eq!(
        save.snapshot_crc,
        max_core::crc::crc16(&app.state().snapshot_bytes())
    );
    assert_eq!(save.snapshot_crc, app.digest().state_digest);

    // Файл: запись и чтение с проверкой CRC.
    let dir = std::env::temp_dir().join(format!("max_sp_save_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join(max_app::local::LOCAL_SAVE_FILE);

    let written = app.write_local_save(&path).expect("файл записан");
    assert_eq!(written, bytes.len());

    let loaded = App::read_local_save(&path).expect("файл прочитан");
    assert_eq!(loaded, save);

    // Испорченный контейнер отклоняется.
    let mut broken = bytes.clone();
    let last = broken.len() - 1;
    broken[last] ^= 0xFF;
    assert!(
        LocalSave::decode(&broken).is_err(),
        "CRC снимка ловит порчу"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// 5. V70-сейв в локальной партии
// ---------------------------------------------------------------------------

#[test]
fn v70_save_loads_into_the_local_game() {
    let Some(mut app) = local_app() else {
        return;
    };

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("assets"));
    app.config_mut().max_res = config.max_res;
    app.config_mut().patches_res = config.patches_res;

    app.start_local_game_from_save(&setup_vs_one_ai(), V70_SAVE)
        .expect("V70-сейв загружается в локальную партию");

    assert!(app.local_active(), "локальная партия из V70");
    assert!(!app.state().network_game, "локальный режим");
    assert!(app.state().turn_counter >= 1);
    assert!(!app.state().unit_ids_in_order().is_empty());

    // Первый ход: доходим до игрока и закрываем ход.
    assert!(
        run_until_player_turn(&mut app, 40_000),
        "локальный цикл доходит до хода игрока"
    );
    let turn = app.state().turn_counter;
    app.local_end_turn();
    assert!(run_until_player_turn(&mut app, 40_000));
    assert!(
        app.state().turn_counter >= turn,
        "счётчик хода не уменьшается"
    );
}

#[test]
fn start_positions_are_the_four_quadrants() {
    assert_eq!(max_app::local::START_POSITIONS[0], Point::new(22, 22));
    assert_eq!(max_app::local::START_POSITIONS[1], Point::new(88, 88));
    assert_ne!(
        max_app::local::START_POSITIONS[0],
        max_app::local::START_POSITIONS[2]
    );
}
