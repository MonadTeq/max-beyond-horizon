//! Слой 2–3 QA: тесты скриптового драйвера (`qa_run`) и golden-сверки.
//!
//! Проверяется:
//!
//! 1. парсер `.qa` понимает **все** команды языка (и отклоняет опечатки);
//! 2. скрипт `qa/sp-full.qa` проходит целиком теми же обработчиками, что у
//!    мыши/клавиатуры (экраны настройки → партия → ходы ИИ → сохранение);
//! 3. упавший `assert` даёт `passed = false` и текст с номером строки —
//!    именно это превращается в exit 1 у `qa_run` в `ci.sh`;
//! 4. `load <V70>` поднимает локальную партию из настоящего сейва игры;
//! 5. PNG-кодек и компаратор golden-кадров работают без внешних крейтов.

use std::path::{Path, PathBuf};

use max_app::qa::{
    parse_script, png, Assertion, Command, QaConfig, QaRunner, ScreenName, Step,
};

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
select 404 242
select unit 0
move 468 306
move grid 24 25
move near
end_turn
exit
";

    let parsed = steps(script);

    assert_eq!(parsed.len(), 37, "все строки-команды разобраны");
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
    let report = runner.run_file(repo_root().join("max-rust/qa/sp-full.qa"));

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
