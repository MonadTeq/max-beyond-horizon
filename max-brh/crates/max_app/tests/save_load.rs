//! P3: сохранение/загрузка V70 из живого `GameState` — сквозные тесты приложения.
//!
//! Проверяется то, что нельзя проверить в `max_core`:
//!
//! 1. партия, сыгранная локальным циклом, записывается настоящим V70-сейвом
//!    (`App::save_v70_to` → `GameState::to_save_bytes`) и **читается обратно**
//!    в свежем `App` (`App::load_v70_path` → `GameState::from_save`);
//! 2. после загрузки совпадают ход, состав/позиции юнитов, золото и настройки;
//! 3. автосейв конца хода создаёт `save10.<ext>` (C++ `SETUP.auto_save`);
//! 4. окно сохранения/загрузки (`saveloadmenu.cpp`) перечисляет слоты с
//!    превью-инфо, листает страницы, принимает имя и клики по кнопкам.

use std::path::{Path, PathBuf};

use max_app::local::LocalSetup;
use max_app::saveload::{SaveLoadAction, SaveLoadMenu, SaveLoadMode};
use max_app::ui::{InputEvent, Key};
use max_app::{App, AppConfig};
use max_core::enums::{PlayMode, VictoryType};
use max_core::save::SaveFile;
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitId;
use max_core::MissionCategory;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// Уникальный временный каталог теста (без внешних крейтов).
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "max_app_saveload_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Приложение с настоящими ассетами (иначе карту не сгенерировать).
fn local_app(save_dir: &Path) -> Option<App> {
    let data = repo_path("max-original");

    if !data.join("MAX.RES").is_file() {
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(&data)
        .with_assets_dir(repo_path("assets"))
        .with_mode(max_app::AppMode::Ui)
        .with_save_dir(save_dir);

    let mut app = App::new(config);
    app.init_assets().ok()?;

    Some(app)
}

/// 1 человек (RED) + 1 компьютер (GREEN), без таймеров (детерминизм).
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
        ..LocalSetup::default()
    }
}

/// Прокручивает локальный цикл до хода человека (не больше `limit` кадров).
fn run_until_player_turn(app: &mut App, limit: usize) {
    for _ in 0..limit {
        if app
            .local()
            .map(|local| local.phase == max_app::local::LocalPhase::PlayerTurn)
            .unwrap_or(false)
        {
            return;
        }
        if app.local().is_none() {
            return;
        }
        app.local_update(0.0);
    }
}

#[test]
fn played_game_saves_as_v70_and_loads_in_a_fresh_app() {
    let dir = temp_dir("roundtrip");
    let Some(mut app) = local_app(&dir) else {
        eprintln!("нет max-original/MAX.RES — тест пропущен");
        return;
    };

    let setup = setup_vs_one_ai();
    app.start_local_game(&setup).expect("локальная партия стартует");
    // Пара кадров хода: состояние не пустое (юниты двигаются/тикают).
    for _ in 0..3 {
        app.local_update(0.0);
    }

    let before = snapshot(&app);
    let path = dir.join("turn1.dta");
    let bytes = app
        .save_v70_to(&path, "QA turn 1")
        .expect("V70-запись удалась");
    assert!(bytes > 10_000, "сейв содержит состояние: {bytes} байт");

    // Файл — настоящий V70: парсится и содержит те же ключевые поля.
    let parsed = SaveFile::parse(&std::fs::read(&path).expect("файл записан")).expect("парсится");
    assert_eq!(parsed.header.save_name(), "QA turn 1");
    assert_eq!(parsed.game.turn_counter, before.turn);
    assert_eq!(parsed.units.len(), before.units.len());

    // Свежий `App` (как после перезапуска клиента) загружает файл.
    let mut fresh = local_app(&dir).expect("ассеты");
    fresh.load_v70_path(&path).expect("V70 загружается");
    let after = snapshot(&fresh);

    assert_eq!(after.turn, before.turn, "ход совпадает");
    assert_eq!(after.units, before.units, "состав юнитов совпадает");
    assert_eq!(after.gold, before.gold, "золото совпадает");
    assert_eq!(after.ids, before.ids, "id и позиции юнитов совпадают");
    assert_eq!(after.team_types, before.team_types, "типы команд совпадают");
    assert!(
        after.units.len() > 1,
        "в партии есть юниты игрока и ИИ: {}",
        after.units.len()
    );

    // Повторный сейв загруженной партии стабилен по ключевым полям.
    let path2 = dir.join("turn1-again.dta");
    fresh.save_v70_to(&path2, "QA turn 1").expect("повторный сейв");
    let reparsed = SaveFile::parse(&std::fs::read(&path2).expect("файл")).expect("парсится");
    assert_eq!(reparsed.game.turn_counter, before.turn);
    assert_eq!(reparsed.units.len(), before.units.len());

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn autosave_is_written_at_the_end_of_a_round() {
    let dir = temp_dir("autosave");
    let Some(mut app) = local_app(&dir) else {
        eprintln!("нет max-original/MAX.RES — тест пропущен");
        return;
    };

    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия стартует");
    let first_turn = app.state().turn_counter;

    // Конец хода человека → ход компьютера → `finish_turn` → автосейв.
    app.local_end_turn();
    run_until_player_turn(&mut app, 20_000);
    assert!(
        app.state().turn_counter > first_turn,
        "раунд завершился: turn={}",
        app.state().turn_counter
    );

    // Категория custom → `dta`, слот автосейва — 10 (C++ `SaveLoad_GetSaveFileName`).
    let autosave = dir.join("save10.dta");
    assert!(
        autosave.is_file(),
        "автосейв {} не создан",
        autosave.display()
    );

    let save = SaveFile::parse(&std::fs::read(&autosave).expect("файл автосейва"))
        .expect("автосейв — валидный V70");
    assert_eq!(save.game.turn_counter, app.state().turn_counter);
    assert_eq!(save.units.len(), app.state().unit_count());
    assert!(
        save.header.save_name().starts_with("(Turn "),
        "имя автосейва как в C++: {:?}",
        save.header.save_name()
    );
    assert!(save.message_logs.iter().any(|log| !log.is_empty()), "журнал UI попал в сейв");

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn save_menu_lists_slots_with_preview_and_edits_the_name() {
    let dir = temp_dir("menu");
    let Some(mut app) = local_app(&dir) else {
        eprintln!("нет max-original/MAX.RES — тест пропущен");
        return;
    };

    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия стартует");
    // Слот 1: пустая категория custom → `save1.dta`.
    app.save_game_to_slot(1, "First save").expect("сейв в слот 1");

    let mut menu = SaveLoadMenu::new(SaveLoadMode::Save, MissionCategory::Custom, &dir);
    assert_eq!(menu.slots.len(), 10, "страница — 10 слотов");
    assert_eq!(menu.slots[0].slot, 1);
    assert_eq!(menu.slots[0].file_name, "save1.dta");
    assert!(menu.slots[0].in_use, "слот 1 занят");
    assert!(!menu.slots[1].in_use, "слот 2 пуст");

    let preview = menu.slots[0].preview.as_ref().expect("превью слота 1");
    assert_eq!(preview.save_name, "First save");
    assert_eq!(preview.turn, app.state().turn_counter);
    assert_eq!(preview.units, app.state().unit_count());
    assert_eq!(preview.version, max_core::SAVE_VERSION_V70);

    // Имя: 30 байт как в V70, Backspace и печатные символы.
    menu.select(0);
    assert_eq!(menu.name, "First save", "имя слота подставляется");
    menu.name.clear();
    for character in "Second".chars() {
        menu.key(Key::Char(character));
    }
    assert_eq!(menu.name, "Second");
    menu.key(Key::Backspace);
    assert_eq!(menu.name, "Secon");

    // Стрелки водят выбор по сетке 2×5.
    menu.key(Key::Down);
    assert_eq!(menu.selected, Some(1));
    menu.key(Key::Right);
    assert_eq!(menu.selected, Some(6));

    // Страницы: 10 слотов на страницу, границы как в C++.
    assert!(menu.page(1));
    assert_eq!(menu.first_slot, 11);
    assert!(menu.page(-1));
    assert_eq!(menu.first_slot, 1);

    // Возвращаем выбор на пустой слот 7 после листания страниц.
    menu.select(6);
    assert_eq!(menu.name, "Secon", "имя пустого слота не перетирается");

    // Клик по кнопке `Cancel` закрывает окно (C++ `MNUBTN4U` в 354,438).
    assert_eq!(menu.click(400, 450), SaveLoadAction::Cancel);
    // Клик по «Load» (C++ `MNUBTN6U` в 514,438): выбран пустой слот 7 — не грузим.
    assert_eq!(menu.click(560, 450), SaveLoadAction::None);
    // Клик по «Save» в режиме сохранения — запись имени в выбранный слот
    // (после стрелок выбран индекс 6 → слот 7).
    assert_eq!(
        menu.click(200, 450),
        SaveLoadAction::Save {
            slot: 7,
            name: "Secon".to_string()
        }
    );

    // Режим загрузки: тот же слот виден и загружается.
    let mut load = SaveLoadMenu::new(SaveLoadMode::Load, MissionCategory::Custom, &dir);
    assert!(load.slots[0].in_use);
    assert!(load.name.is_empty(), "в режиме загрузки имя не редактируется");
    load.select(0);
    assert_eq!(load.load_action(), SaveLoadAction::Load { slot: 1 });
    load.select(1);
    assert_eq!(load.load_action(), SaveLoadAction::None, "пустой слот не грузится");

    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn save_hotkey_opens_and_closes_the_menu_in_a_local_game() {
    let dir = temp_dir("hotkey");
    let Some(mut app) = local_app(&dir) else {
        eprintln!("нет max-original/MAX.RES — тест пропущен");
        return;
    };

    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия стартует");
    run_until_player_turn(&mut app, 2_000);

    assert!(!app.open_save_menu(true), "в сети сохранение недоступно");
    assert!(app.open_save_menu(false), "в локальной партии — доступно");
    assert!(app.save_load().is_some());

    // Ввод уходит в окно: Esc закрывает, состояние партии не меняется.
    let turn = app.state().turn_counter;
    max_app::ui::handle_session_input(&mut app, InputEvent::Key(Key::Escape), false);
    assert!(app.save_load().is_none(), "Esc закрывает окно");
    assert_eq!(app.state().turn_counter, turn, "Esc не двигает партию");

    assert!(app.open_load_menu(false));
    assert_eq!(
        app.save_load().map(|menu| menu.mode),
        Some(SaveLoadMode::Load)
    );

    std::fs::remove_dir_all(dir).ok();
}

/// Снимок ключевых полей состояния для сравнения «до/после загрузки».
struct Snapshot {
    turn: i32,
    units: Vec<UnitId>,
    ids: Vec<(UnitId, max_core::UnitType, i16, i16)>,
    gold: u32,
    team_types: [TeamKind; 4],
}

fn snapshot(app: &App) -> Snapshot {
    let state = app.state();
    let ids: Vec<UnitId> = state.unit_ids_in_order();
    let mut detailed: Vec<(UnitId, max_core::UnitType, i16, i16)> = ids
        .iter()
        .filter_map(|id| {
            state
                .unit(*id)
                .map(|unit| (*id, unit.unit_type, unit.grid.x, unit.grid.y))
        })
        .collect();
    detailed.sort_by_key(|entry| entry.0);

    Snapshot {
        turn: state.turn_counter,
        units: ids,
        ids: detailed,
        gold: state.team_units[TeamId::RED.0 as usize].gold,
        team_types: std::array::from_fn(|index| state.teams[index].team_type),
    }
}
