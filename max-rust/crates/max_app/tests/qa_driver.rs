//! Слой 2–3 QA: тесты скриптового драйвера (`qa_run`) и golden-сверки.
//!
//! Проверяется:
//!
//! 1. парсер `.qa` понимает **все** команды языка (и отклоняет опечатки);
//! 2. скрипт `max-rust-qa/scripts/sp-full.qa` проходит целиком теми же
//!    обработчиками, что у мыши/клавиатуры (экраны настройки → партия → ходы
//!    ИИ → сохранение);
//! 3. упавший `assert` даёт `passed = false` и текст с номером строки —
//!    именно это превращается в exit 1 у `qa_run` в `ci.sh`;
//! 4. `load <V70>` поднимает локальную партию из настоящего сейва игры;
//! 5. PNG-кодек и компаратор golden-кадров работают без внешних крейтов.

use std::path::{Path, PathBuf};

use max_app::hud;
use max_app::menu;
use max_app::qa::{
    parse_script, png, Assertion, Cmp, Command, QaConfig, QaMenuClient, QaRunner, RecordContext,
    Recorder, ScreenName, Step,
};
use max_app::setup;
use max_app::ui::{handle_session_input, InputEvent, Screen, SessionRequest, Transition, UiContext};
use max_app::{App, AppConfig, AppMode};
use max_core::geo::{Point, MAP_HEIGHT, MAP_WIDTH};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn has_game_data() -> bool {
    repo_root().join("max-original/MAX.RES").is_file()
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "max_qa_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));

    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn runner(out_dir: &Path) -> QaRunner {
    QaRunner::new(QaConfig::new(out_dir).with_frame_limit(20_000)).expect("QA-драйвер стартует")
}

fn steps(source: &str) -> Vec<Step> {
    parse_script(source).expect("скрипт разбирается")
}

// ---------------------------------------------------------------------------
// 1. Парсер
// ---------------------------------------------------------------------------

#[test]
fn parser_handles_every_command_of_the_language() {
    let script = "\
# все команды языка .qa
click 10 20
rclick 30 40
key ENTER
key t
type \"hello world\"
wait 5
tick 7
mem
shot frame
assert turn=1
assert team=RED
assert units>=2
assert screen=InGame
assert phase=PlayerTurn
assert notice=GREEN
assert digest=0xBEEF
assert victory
assert defeat
assert selected
save slot.mxs
load SAVE1.CAM
menu new_game
menu multiplayer
menu hot_seat
menu join
setup done
setup cancel
setup random
players 0 human
players 1 computer
players 2 none
mobilize buy 0
mobilize done
landing grid 22 22
landing done
select 404 242
select unit 0
move 468 306
move grid 24 25
move near
end_turn
exit
";

    let parsed = steps(script);

    assert_eq!(parsed.len(), 42, "все строки-команды разобраны");
    assert_eq!(parsed[0].command, Command::Click(10, 20));
    assert_eq!(parsed[1].command, Command::RightClick(30, 40));
    assert!(parsed
        .iter()
        .any(|step| step.command == Command::Assert(Assertion::Screen(ScreenName::InGame))));
    assert!(parsed
        .iter()
        .any(|step| step.command == Command::MoveNear));
    assert!(parsed.iter().any(|step| step.command == Command::EndTurn));
    assert_eq!(parsed.last().map(|step| step.command.clone()), Some(Command::Exit));
}

#[test]
fn parser_reports_the_line_of_a_typo() {
    let error = parse_script("click 1 2\n\nassert turn=abc\n").expect_err("опечатка");

    assert!(error.contains("строка 3"), "{error}");
}

// ---------------------------------------------------------------------------
// 2. Скрипт sp-full.qa проходит целиком
// ---------------------------------------------------------------------------

#[test]
fn sp_full_script_passes_through_the_real_input_path() {
    if !has_game_data() {
        return;
    }

    let dir = temp_dir("sp_full");
    let mut runner = runner(&dir);
    let report = runner.run_file(repo_root().join("max-rust-qa/scripts/sp-full.qa"));

    assert!(
        report.passed,
        "sp-full.qa упал: {:?}\n{}",
        report.failures, report.log
    );
    assert!(
        report.frames > 100,
        "партия должна прокрутить кадры (ходы ИИ): {}",
        report.frames
    );

    // Кадры ключевых шагов на месте.
    for name in [
        "main-menu",
        "options",
        "planet",
        "players",
        "game-start",
        "game-selected",
        "game-moved",
    ] {
        let path = dir.join(format!("{name}.png"));
        assert!(path.is_file(), "нет кадра {name}: {}", path.display());
    }

    // Сохранение локальной партии (MXLS) записано рядом с кадрами.
    assert!(dir.join("sp-full.mxs").is_file(), "save sp-full.mxs");

    // Состояние действительно доиграно: три хода и ходы ИИ позади.
    assert!(
        runner.app().state().turn_counter >= 3,
        "turn_counter={}",
        runner.app().state().turn_counter
    );

    // log.txt пишется и содержит итог.
    let log = report.write_log(&dir).expect("лог записан");
    let text = std::fs::read_to_string(log).expect("лог читается");
    assert!(text.contains("RESULT: PASS"), "{text}");

    std::fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------------------
// 3. Упавший ассерт — это провал прогона (в CLI → exit 1)
// ---------------------------------------------------------------------------

#[test]
fn failing_assert_fails_the_run_and_names_the_line() {
    let dir = temp_dir("assert_fail");
    let mut runner = runner(&dir);

    // Стартовый экран — MainMenu: проверка InGame обязана упасть.
    let report = runner.run(
        &steps("assert screen=InGame\nassert screen=MainMenu\n"),
        "inline",
    );

    assert!(!report.passed, "падение ассерта — не PASS");
    assert_eq!(report.failures.len(), 1);
    assert!(
        report.failures[0].starts_with("строка 1:"),
        "{:?}",
        report.failures
    );
    assert!(
        report.failures[0].contains("assert screen=InGame"),
        "{:?}",
        report.failures
    );
    assert!(report.log.contains("FAIL"), "{}", report.log);
    // После падения шаги не выполняются: второго ассерта в логе нет.
    assert!(!report.log.contains("screen=MainMenu:"), "{}", report.log);

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn unknown_command_is_a_script_error_not_a_panic() {
    let dir = temp_dir("bad_command");
    let script = dir.join("bogus.qa");
    std::fs::write(&script, "bogus\n").expect("скрипт записан");

    let mut runner = runner(&dir);
    let report = runner.run_file(&script);

    assert!(!report.passed);
    assert!(report.failures[0].contains("bogus"), "{:?}", report.failures);
    assert!(report.log.contains("bogus"), "{}", report.log);

    std::fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------------------
// 4. load V70 → локальная партия
// ---------------------------------------------------------------------------

#[test]
fn v70_save_is_loaded_by_the_script_command() {
    if !repo_root().join("max-original/SAVE1.CAM").is_file() {
        return;
    }

    let dir = temp_dir("load_v70");
    let mut runner = runner(&dir);
    let report = runner.run(
        &steps(
            "load SAVE1.CAM\nassert screen=InGame\nassert units>=1\ntick 5\nassert turn>=1\nsave loaded.mxs\n",
        ),
        "load",
    );

    assert!(report.passed, "{:?}\n{}", report.failures, report.log);
    assert!(dir.join("loaded.mxs").is_file(), "save после load");
    assert!(runner.app().local_active(), "локальная партия поднялась");

    std::fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------------------
// 5. PNG и golden-сверка
// ---------------------------------------------------------------------------

#[test]
fn golden_pngs_roundtrip_and_compare_without_drift() {
    let dir = temp_dir("golden_png");
    let mut runner = runner(&dir);

    let report = runner.run(&steps("shot menu\nshot menu\n"), "shot");
    assert!(report.passed, "{:?}", report.failures);

    let shot = dir.join("menu.png");
    let image = png::read(&shot).expect("кадр читается");

    assert_eq!((image.width, image.height), (640, 480));
    assert!(std::fs::metadata(&shot).expect("метаданные").len() < 300_000);

    // Повторный encode → decode не меняет пиксели.
    let again = png::decode(&png::encode(&image)).expect("round trip");
    assert_eq!(pixel_hash(&again), pixel_hash(&image));

    let diff = png::compare(&image, &again, 30).expect("сверка");
    assert_eq!(diff.changed, 0);
    assert_eq!(diff.changed_ratio(), 0.0);

    std::fs::remove_dir_all(dir).ok();
}

fn pixel_hash(image: &png::Image) -> u32 {
    png::crc32(&image.rgb)
}

// ---------------------------------------------------------------------------
// 6. Слой 2.5: запись ручной игры → .qa → qa_run до того же состояния
// ---------------------------------------------------------------------------

fn click_rect(rect: max_render::buffer::PixelRect) -> InputEvent {
    InputEvent::MouseClick(rect.ulx + rect.width / 2, rect.uly + rect.height / 2)
}

/// Гоняет тот же путь игрока, что `QaRunner`, но с ручным `InputEvent`-потоком,
/// который одновременно уходит в [`Recorder`].
struct ManualPlay {
    app: App,
    ctx: UiContext,
    screen: Screen,
    recorder: Recorder,
}

impl ManualPlay {
    fn new(out_dir: &Path) -> Self {
        let config = AppConfig::default()
            .with_data_dir(repo_root().join("max-original"))
            .with_assets_dir(repo_root().join("assets"))
            .with_mode(AppMode::Ui);
        let mut app = App::new(config);
        app.init_assets().expect("ассеты для ручной игры");

        let mut ctx = UiContext::new(Box::new(QaMenuClient), 640, 480);
        ctx.player_name = "Commander".to_string();
        ctx.world_dir = repo_root().join("max-original");

        let recorder = Recorder::new(out_dir.join("recorded.qa"))
            .expect("файл записи")
            .with_pause_cap(40_000);

        Self {
            app,
            ctx,
            screen: Screen::MainMenu,
            recorder,
        }
    }

    fn context(&self) -> RecordContext {
        RecordContext {
            in_session: matches!(self.screen, Screen::InSession { .. }),
            text_open: self.app.hud().chat.is_active(),
        }
    }

    /// Ровно `main.rs`: запись → экран/HUD → переход.
    fn send(&mut self, event: InputEvent) {
        self.recorder.record(event, self.context());

        if matches!(self.screen, Screen::InSession { .. }) {
            match handle_session_input(&mut self.app, event, false) {
                Transition::None => {}
                Transition::LeaveSession => {
                    self.screen = Screen::MainMenu;
                    self.app.leave_local_game();
                }
                Transition::Quit => {}
                Transition::StartLocal(_) | Transition::StartSession(_) => {
                    panic!("ввод в сессии не меняет режим")
                }
            }

            return;
        }

        match self.screen.handle_input(&mut self.ctx, event) {
            Transition::None => {}
            Transition::StartLocal(setup) => {
                self.app.start_local_game(&setup).expect("локальная партия");
                self.screen = Screen::InSession {
                    request: SessionRequest {
                        game_id: "local".to_string(),
                        password: None,
                    },
                };
                self.ctx.status = String::new();
            }
            Transition::LeaveSession => {
                self.screen = Screen::MainMenu;
                self.app.leave_local_game();
            }
            Transition::Quit => {}
            Transition::StartSession(_) => panic!("сеть в тесте записи не поднимается"),
        }
    }

    /// Кадр игрового цикла и счётчик рекордера (`main.rs`).
    fn frame(&mut self) {
        self.recorder.begin_frame();

        if matches!(self.screen, Screen::InSession { .. }) && self.app.local_active() {
            let update = self.app.local_update(0.0);

            if update.finished || self.app.local().is_some_and(|local| local.finished()) {
                self.screen = Screen::GameOver;
            }
        }

        if let Screen::LocalSetup(form) = &mut self.screen {
            let palette = self
                .app
                .assets()
                .and_then(|assets| assets.menu.as_ref())
                .map(|menu| menu.palette);
            form.ensure_minimaps(palette.as_ref());
        }
    }

    fn ready(&self) -> bool {
        match self.app.local() {
            None => true,
            Some(local) => local.finished() || local.notice.is_some() || !local.input_blocked(),
        }
    }

    /// `wait` в языке `.qa`: крутим кадры, пока игра не готова к вводу.
    fn wait_ready(&mut self) {
        while !self.ready() {
            self.frame();
        }
    }
}

fn visible_point(app: &App, grid: Point) -> Option<(i32, i32)> {
    let (x, y) = hud::tile_center(&app.map_viewport(), grid);
    let (x, y) = (
        x + max_render::layout::MAIN_MAP.ulx,
        y + max_render::layout::MAIN_MAP.uly,
    );

    max_render::layout::MAIN_MAP
        .contains(x, y)
        .then_some((x, y))
}

/// Юнит игрока, который виден при текущей камере (камеру не двигаем, иначе
/// записанный клик не совпадёт с реплеем).
fn visible_player_unit(app: &App) -> Option<((i32, i32), Point)> {
    let team = app.player_team();

    app.state()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state().unit(id))
        .filter(|unit| unit.team == team)
        .find_map(|unit| visible_point(app, unit.grid).map(|point| (point, unit.grid)))
}

/// Свободная соседняя клетка, видимая в текущей камере (порт `move near`).
fn visible_neighbor(app: &App, grid: Point) -> Option<(i32, i32)> {
    for (dx, dy) in [
        (1, 0),
        (-1, 0),
        (0, 1),
        (0, -1),
        (1, 1),
        (-1, -1),
        (1, -1),
        (-1, 1),
    ] {
        let target = Point::new(grid.x + dx, grid.y + dy);

        if target.x < 0 || target.y < 0 || target.x >= MAP_WIDTH || target.y >= MAP_HEIGHT {
            continue;
        }
        if !app.state().units_at(target).is_empty() {
            continue;
        }
        if let Some(point) = visible_point(app, target) {
            return Some(point);
        }
    }

    None
}

#[test]
fn recorded_manual_session_round_trips_through_qa_run() {
    if !has_game_data() {
        return;
    }

    let dir = temp_dir("record_roundtrip");
    let mut play = ManualPlay::new(&dir);

    // --- Главное меню → New Game → Options/Planet/Players --------------------
    play.send(click_rect(menu::MAIN_MENU_BUTTONS[0].rect));
    assert!(matches!(&play.screen, Screen::LocalSetup(_)));

    // Options: No Limit + Turn Based (те же точки, что в sp-full.qa), Done.
    play.send(InputEvent::MouseClick(240, 110));
    play.send(InputEvent::MouseClick(500, 100));
    play.send(InputEvent::MouseClick(516, 440));
    assert!(matches!(&play.screen, Screen::LocalSetup(_)));

    // Planet: Done.
    play.send(InputEvent::MouseClick(516, 440));
    assert!(matches!(&play.screen, Screen::LocalSetup(_)));

    // Players: RED — человек, GREEN — компьютер, Done → мобилизация.
    play.send(click_rect(setup::player_type_button_rect(0, 0)));
    play.send(click_rect(setup::player_type_button_rect(1, 1)));
    play.send(InputEvent::MouseClick(516, 440));
    assert!(
        matches!(&play.screen, Screen::PreGame(_)),
        "Done на Players открывает фазу мобилизации: {:?}",
        play.ctx.status
    );

    // Мобилизация: `Done` (`BLDONE_U` 54×23 в 447,452).
    play.send(InputEvent::MouseClick(447 + 27, 452 + 11));
    assert!(
        matches!(&play.screen, Screen::PreGame(_)),
        "после мобилизации — выбор точки высадки"
    );

    // Выбор точки: клетка 22,22 (суша Snowcrab) и `Done` (514,438,109,40).
    play.send(InputEvent::MouseClick(180 + 22 * 4 + 2, 18 + 22 * 4 + 2));
    play.send(InputEvent::MouseClick(568, 458));
    assert!(
        matches!(&play.screen, Screen::InSession { .. }),
        "Done на высадке должен начать партию: {:?}",
        play.ctx.status
    );

    play.wait_ready();

    // --- Партия: select → move → end_turn -----------------------------------
    let ((select_x, select_y), grid) =
        visible_player_unit(&play.app).expect("в стартовой камере есть юнит RED");
    play.send(InputEvent::MouseClick(select_x, select_y));
    assert!(play.app.hud().selected.is_some(), "клик должен выделить юнита");

    if let Some((move_x, move_y)) = visible_neighbor(&play.app, grid) {
        play.send(InputEvent::RightClick(move_x, move_y));
    }

    play.send(click_rect(max_render::layout::END_TURN_BUTTON));
    // Ход ИИ: ровно те кадры, которые реплей прокрутит командой wait.
    play.wait_ready();

    let recorded_digest = play.app.digest();
    let record_path = play
        .recorder
        .finish()
        .expect("финализация записи")
        .expect("путь записи");

    let script = std::fs::read_to_string(&record_path).expect("запись читается");
    let parsed = parse_script(&script).expect("записанный скрипт разбирается");

    assert!(
        parsed.len() >= 9,
        "слишком короткий сценарий ({} шагов):\n{script}",
        parsed.len()
    );
    assert!(
        parsed
            .iter()
            .any(|step| matches!(step.command, Command::Click(..))),
        "{script}"
    );
    assert!(
        parsed
            .iter()
            .any(|step| matches!(step.command, Command::RightClick(..))),
        "{script}"
    );
    assert!(
        parsed
            .iter()
            .any(|step| matches!(step.command, Command::Wait(_))),
        "в записи должен быть wait хода ИИ:\n{script}"
    );

    // --- Реплей тем же qa_run-харнессом -------------------------------------
    let replay_dir = temp_dir("record_replay");
    let mut runner =
        QaRunner::new(QaConfig::new(&replay_dir).with_frame_limit(40_000)).expect("qa_run");

    let report = runner.run_file(&record_path);

    assert!(
        report.passed,
        "записанный скрипт упал: {:?}\n{}",
        report.failures, report.log
    );
    assert!(
        matches!(runner.screen(), Screen::InSession { .. }),
        "реплей должен остаться в партии"
    );
    assert_eq!(
        runner.app().digest(),
        recorded_digest,
        "реплей записанного сценария должен прийти в то же состояние:\n{script}"
    );

    std::fs::remove_dir_all(dir).ok();
    std::fs::remove_dir_all(replay_dir).ok();
}

// ---------------------------------------------------------------------------
// 7. P4: soak/edge-сценарии и команда `mem`
// ---------------------------------------------------------------------------

#[test]
fn mem_command_writes_rss_to_the_log() {
    let dir = temp_dir("mem");
    let mut runner = runner(&dir);

    let report = runner.run(&steps("mem\nassert screen=MainMenu\nmem\n"), "mem");

    assert!(report.passed, "{:?}\n{}", report.failures, report.log);
    assert!(report.log.contains("# mem start:"), "{}", report.log);
    assert!(report.log.contains("# mem end:"), "{}", report.log);
    assert!(report.log.contains("mem -> ok (rss="), "{}", report.log);

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn soak_script_is_100_turns_with_periodic_checks() {
    let text = std::fs::read_to_string(repo_root().join("max-rust-qa/scripts/soak-100.qa"))
        .expect("soak-100.qa читается");
    let parsed = parse_script(&text).expect("soak-100.qa разбирается");

    let end_turns = parsed
        .iter()
        .filter(|step| step.command == Command::EndTurn)
        .count();
    let shots = parsed
        .iter()
        .filter(|step| matches!(step.command, Command::Shot(_)))
        .count();

    assert_eq!(end_turns, 100, "soak обязан закрыть 100 ходов");
    assert!(shots >= 5, "кадры каждые 20 ходов: {shots}");
    assert!(
        parsed.iter().any(|step| step.command == Command::Mem),
        "память должна попадать в лог"
    );
    assert!(
        parsed
            .iter()
            .any(|step| step.command == Command::Assert(Assertion::Turn(Cmp::Eq, 101))),
        "финальный assert turn=101"
    );
    assert!(
        parsed
            .iter()
            .any(|step| step.command == Command::SaveV70("soak-turn100.dta".to_string())),
        "P3-сейв на 100-м ходу"
    );
}

#[test]
fn edge_cases_script_passes_click_spam_and_unicode() {
    if !has_game_data() {
        return;
    }

    let dir = temp_dir("edge_cases");
    let mut runner = runner(&dir);
    let report = runner.run_file(repo_root().join("max-rust-qa/scripts/edge-cases.qa"));

    assert!(
        report.passed,
        "edge-cases.qa упал: {:?}\n{}",
        report.failures, report.log
    );
    assert!(report.frames > 0, "партия прокручивала кадры");
    assert!(
        runner.app().state().turn_counter >= 2,
        "второй старт доигран до хода 2: turn={}",
        runner.app().state().turn_counter
    );

    for name in [
        "edge-menu-spam",
        "edge-game-spam",
        "edge-unicode-chat",
        "edge-unicode-name",
        "edge-repeat-start",
        "edge-second-game",
    ] {
        let path = dir.join(format!("{name}.png"));
        assert!(path.is_file(), "нет кадра {name}: {}", path.display());
    }

    std::fs::remove_dir_all(dir).ok();
}
