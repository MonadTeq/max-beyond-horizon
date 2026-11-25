//! P3: V70 save/load from the live `GameState` — end-to-end application tests.
//!
//! What cannot be checked in `max_core` is checked:
//!
//! 1. a match played by the local loop is written as a real V70 save
//!    (`App::save_v70_to` → `GameState::to_save_bytes`) and **read back**
//!    into a fresh `App` (`App::load_v70_path` → `GameState::from_save`);
//! 2. after loading the turn, unit composition/positions, gold and settings match;
//! 3. the end-of-turn autosave creates `save10.<ext>` (C++ `SETUP.auto_save`);
//! 4. the save/load window (`saveloadmenu.cpp`) lists slots with
//!    preview info, pages, accepts a name and button clicks.

use std::path::{Path, PathBuf};

use max_app::local::LocalSetup;
use max_app::saveload::{save_seed, SaveConfirmAction, SaveLoadAction, SaveLoadMenu, SaveLoadMode};
use max_app::ui::{InputEvent, Key};
use max_app::{App, AppConfig};
use max_core::enums::{PlayMode, VictoryType};
use max_core::rng::Randomizer;
use max_core::save::SaveFile;
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitId;
use max_core::universe::GameUniverse;
use max_core::MissionCategory;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// Unique temporary test directory (without external crates).
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

/// Application with real assets (otherwise the map cannot be generated).
fn local_app(save_dir: &Path) -> Option<App> {
    local_app_split(save_dir, None)
}

/// The same, with an explicit `FilePathGameData` save directory (S1.4).
fn local_app_split(pref_dir: &Path, data_save_dir: Option<&Path>) -> Option<App> {
    let data = repo_path("max-original");

    if !data.join("MAX.RES").is_file() {
        return None;
    }

    let mut config = AppConfig::default()
        .with_data_dir(&data)
        .with_assets_dir(repo_path("assets"))
        .with_mode(max_app::AppMode::Ui)
        .with_save_dir(pref_dir);
    if let Some(data_save_dir) = data_save_dir {
        config = config.with_save_data_dir(data_save_dir);
    }

    let mut app = App::new(config);
    app.init_assets().ok()?;

    Some(app)
}

/// 1 human (RED) + 1 computer (GREEN), no timers (determinism).
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

/// Runs the local loop until the human turn (at most `limit` frames).
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
    // A couple of turn frames: the state is not empty (units move/tick).
    for _ in 0..3 {
        app.local_update(0.0);
    }

    let before = snapshot(&app);
    let path = dir.join("turn1.dta");
    let bytes = app
        .save_v70_to(&path, "QA turn 1")
        .expect("V70-запись удалась");
    assert!(bytes > 10_000, "сейв содержит состояние: {bytes} байт");

    // The file is a real V70: it parses and contains the same key fields.
    let parsed = SaveFile::parse(&std::fs::read(&path).expect("файл записан")).expect("парсится");
    assert_eq!(parsed.header.save_name(), "QA turn 1");
    assert_eq!(parsed.game.turn_counter, before.turn);
    assert_eq!(parsed.units.len(), before.units.len());

    // A fresh `App` (like after a client restart) loads the file.
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

    // A repeated save of the loaded match is stable in the key fields.
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
    let first_turn = app.state_ref().turn_counter();

    // Human End Turn → computer turn → `finish_turn` → autosave.
    app.local_end_turn();
    run_until_player_turn(&mut app, 20_000);
    assert!(
        app.state_ref().turn_counter() > first_turn,
        "раунд завершился: turn={}",
        app.state_ref().turn_counter()
    );

    // Custom category → `dta`, the autosave slot — 10 (C++ `SaveLoad_GetSaveFileName`).
    let autosave = dir.join("save10.dta");
    assert!(
        autosave.is_file(),
        "автосейв {} не создан",
        autosave.display()
    );

    let save = SaveFile::parse(&std::fs::read(&autosave).expect("файл автосейва"))
        .expect("автосейв — валидный V70");
    assert_eq!(save.game.turn_counter, app.state_ref().turn_counter());
    assert_eq!(save.units.len(), app.state_ref().unit_count());
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
    // Slot 1: empty custom category → `save1.dta`.
    app.save_game_to_slot(1, "First save").expect("сейв в слот 1");

    let mut menu = SaveLoadMenu::new(SaveLoadMode::Save, MissionCategory::Custom, &dir);
    assert_eq!(menu.slots.len(), 10, "страница — 10 слотов");
    assert_eq!(menu.slots[0].slot, 1);
    assert_eq!(menu.slots[0].file_name, "save1.dta");
    assert!(menu.slots[0].in_use, "слот 1 занят");
    assert!(!menu.slots[1].in_use, "слот 2 пуст");

    let preview = menu.slots[0].preview.as_ref().expect("превью слота 1");
    assert_eq!(preview.save_name, "First save");
    assert_eq!(preview.turn, app.state_ref().turn_counter());
    assert_eq!(preview.units, app.state_ref().unit_count());
    assert_eq!(preview.version, max_core::SAVE_VERSION_V70);

    // Name: 30 bytes as in V70, Backspace and printable characters.
    menu.select(0);
    assert_eq!(menu.name, "First save", "имя слота подставляется");
    menu.name.clear();
    for character in "Second".chars() {
        menu.key(Key::Char(character));
    }
    assert_eq!(menu.name, "Second");
    menu.key(Key::Backspace);
    assert_eq!(menu.name, "Secon");

    // The arrows drive the selection over the 2×5 grid.
    menu.key(Key::Down);
    assert_eq!(menu.selected, Some(1));
    menu.key(Key::Right);
    assert_eq!(menu.selected, Some(6));

    // Pages: 10 slots per page, boundaries as in C++.
    assert!(menu.page(1));
    assert_eq!(menu.first_slot, 11);
    assert!(menu.page(-1));
    assert_eq!(menu.first_slot, 1);

    // Return the selection to the empty slot 7 after paging.
    menu.select(6);
    assert_eq!(menu.name, "Secon", "имя пустого слота не перетирается");

    // A click on the `Cancel` button closes the window (C++ `MNUBTN4U` at 354,438).
    assert_eq!(menu.click(400, 450), SaveLoadAction::Cancel);
    // A click on "Load" (C++ `MNUBTN6U` at 514,438): empty slot 7 is selected — we do not load.
    assert_eq!(menu.click(560, 450), SaveLoadAction::None);
    // A click on "Save" in save mode — writing the name into the selected slot
    // (after the arrows index 6 is selected → slot 7).
    assert_eq!(
        menu.click(200, 450),
        SaveLoadAction::Save {
            slot: 7,
            name: "Secon".to_string()
        }
    );

    // Load mode: the same slot is visible and loads.
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

    assert!(!app.open_save_menu(max_app::SessionChannel::Lockstep), "в сети сохранение недоступно");
    assert!(app.open_save_menu(max_app::SessionChannel::Local), "в локальной партии — доступно");
    assert!(app.save_load().is_some());

    // Input goes to the window: Esc closes it, the match state does not change.
    let turn = app.state_ref().turn_counter();
    max_app::ui::handle_session_input(&mut app, InputEvent::Key(Key::Escape));
    assert!(app.save_load().is_none(), "Esc закрывает окно");
    assert_eq!(app.state_ref().turn_counter(), turn, "Esc не двигает партию");

    assert!(app.open_load_menu(max_app::SessionChannel::Local));
    assert_eq!(
        app.save_load().map(|menu| menu.mode),
        Some(SaveLoadMode::Load)
    );

    std::fs::remove_dir_all(dir).ok();
}

/// Load-path gate (ADR `game-universe-facade.md` §1 item 6): a local match loaded
/// from V70 resumes at the saved active team and does **not** re-apply the economy
/// the turn already had before the save (`GAME_STATE_10_LOAD_GAME`,
/// `game_manager.cpp:458-480`).
///
/// The marker is a `CONSTRCT` with a `Build` order: every `manage_economy` pass
/// charges `Cargo_GetRawConsumptionRate` from its storage
/// (`UnitInfo::UpdateProduction`), so a second pass is visible as `40 → 38`.
#[test]
fn local_load_resumes_without_replaying_the_turn_economy() {
    let dir = temp_dir("local_resume");
    let Some(mut app) = local_app(&dir) else {
        eprintln!("нет max-original/MAX.RES — тест пропущен");
        return;
    };

    let setup = setup_vs_one_ai();
    app.start_local_game(&setup)
        .expect("локальная партия стартует");
    assert_eq!(
        app.local().map(|local| local.phase),
        Some(max_app::local::LocalPhase::PlayerTurn),
        "RED's turn is active: the round entry already applied its economy"
    );
    let saved_active = app.state_ref().active_turn_team();
    let saved_turn = app.state_ref().turn_counter();

    // The builder is injected after the round entry, so its storage records the
    // number of economy passes after the save only.
    let builder = app
        .universe_mut()
        .spawn_unit(
            max_core::UnitType::Constructor,
            TeamId::RED,
            max_core::geo::Point::new(30, 30),
            0,
        )
        .expect("builder spawns");
    {
        let mut unit = app.state_ref().unit(builder).expect("builder").clone();
        unit.storage = 40;
        unit.build_list = vec![max_core::UnitType::TrainingHall];
        unit.build_time = 10;
        unit.build_rate = 1;
        unit.orders = max_core::UnitOrder::Build;
        unit.state = max_core::UnitOrderState::ExecutingOrder;
        app.universe_mut().write_unit(builder, unit);
    }

    let bytes = app
        .save_v70_bytes("resume gate")
        .expect("V70 write of the live match");

    let mut fresh = local_app(&dir).expect("ассеты");
    fresh
        .start_local_game_from_save(&setup, &bytes)
        .expect("V70 loads as a local match");

    assert_eq!(
        fresh.state_ref().active_turn_team(),
        saved_active,
        "the loaded match resumes at the saved active team"
    );
    assert_eq!(fresh.state_ref().turn_counter(), saved_turn);
    assert_eq!(
        fresh.local().map(|local| local.phase),
        Some(max_app::local::LocalPhase::PlayerTurn),
        "the resumed turn is the human turn, not a round close"
    );
    assert_eq!(
        fresh.state_ref().unit(builder).expect("builder").storage,
        40,
        "the loaded turn applied manage_economy a second time"
    );

    std::fs::remove_dir_all(dir).ok();
}

/// Snapshot of the key state fields for the "before/after load" comparison.
struct Snapshot {
    turn: i32,
    units: Vec<UnitId>,
    ids: Vec<(UnitId, max_core::UnitType, i16, i16)>,
    gold: u32,
    team_types: [TeamKind; 4],
}

fn snapshot(app: &App) -> Snapshot {
    let state = app.state_ref();
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
        turn: state.turn_counter(),
        units: ids,
        ids: detailed,
        gold: state.team_units()[TeamId::RED.0 as usize].gold,
        team_types: std::array::from_fn(|index| state.teams()[index].team_type),
    }
}

// ---------------------------------------------------------------------------
// S1.1 — save-time RNG seed
// ---------------------------------------------------------------------------

/// The seed is written byte-exactly in the V70 header and re-seeds the generator
/// on load; it reaches the snapshot/CRC through the `Randomizer` state.
#[test]
fn save_seed_is_byte_exact_and_reseeds_the_generator() {
    let dir = temp_dir("seed");
    let Some(mut app) = local_app(&dir) else {
        eprintln!("нет max-original/MAX.RES — тест пропущен");
        return;
    };
    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия стартует");
    for _ in 0..3 {
        app.local_update(0.0);
    }

    // `SaveLoadMenu_Save` (`saveloadmenu.cpp:611-619`): seed = base + Σ file bytes;
    // the port's deterministic base is the live RNG word (owner decision S1.1).
    let rng_word = app.state_ref().rng().state()[0];
    let path = dir.join("save3.dta");
    app.save_v70_to(&path, "Seeded")
        .expect("V70-запись удалась");

    let bytes = std::fs::read(&path).expect("файл записан");
    let parsed = SaveFile::parse(&bytes).expect("парсится");
    let expected = save_seed("save3.dta", rng_word);
    assert_eq!(
        parsed.header.rng_seed, expected,
        "seed = RNG base + Σ bytes(file name)"
    );

    // Byte-exact header round-trip: a parse → write cycle keeps the seed.
    assert_eq!(
        parsed.to_bytes().expect("перезапись"),
        bytes,
        "V70 round-trip is byte-identical"
    );

    // Loading re-seeds `Randomizer` from the header (`saveload.cpp:776`).
    let loaded = GameUniverse::from_save(&parsed);
    assert_eq!(
        loaded.snapshot().rng().state(),
        Randomizer::new(expected).state()
    );

    // The seed is simulation input: another header seed → another snapshot/CRC.
    let mut changed = parsed.clone();
    changed.header.rng_seed = expected.wrapping_add(1);
    let other = GameUniverse::from_save(&changed);
    assert_ne!(
        loaded.snapshot().state_digest(),
        other.snapshot().state_digest(),
        "the seed must change the state CRC through the RNG state"
    );
    assert_ne!(
        loaded.snapshot().snapshot_bytes(),
        other.snapshot().snapshot_bytes()
    );

    std::fs::remove_dir_all(dir).ok();
}

/// The same state saved twice writes the same seed (deterministic QA), while a
/// different slot file name changes it (the C++ file-name addition).
#[test]
fn save_seed_is_deterministic_across_saves() {
    let dir = temp_dir("seed_repeat");
    let Some(mut app) = local_app(&dir) else {
        eprintln!("нет max-original/MAX.RES — тест пропущен");
        return;
    };
    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия стартует");

    let first = app.save_v70_bytes_for("Same", "save1.dta").expect("байты");
    let second = app.save_v70_bytes_for("Same", "save1.dta").expect("байты");
    assert_eq!(first, second, "same state + same slot → same bytes");

    let other_slot = app.save_v70_bytes_for("Same", "save2.dta").expect("байты");
    let a = SaveFile::parse(&first).expect("парсится");
    let b = SaveFile::parse(&other_slot).expect("парсится");
    assert_ne!(
        a.header.rng_seed, b.header.rng_seed,
        "the slot file name contributes to the seed like in C++"
    );

    std::fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------------------
// S1.2 — `.bak` backup
// ---------------------------------------------------------------------------

/// A rewritten autosave keeps the previous slot as `save10.bak`; there is no
/// rotation to `.ba1` (owner decision S1.2).
#[test]
fn overwritten_autosave_keeps_the_previous_slot_as_bak() {
    let dir = temp_dir("backup");
    let Some(mut app) = local_app(&dir) else {
        eprintln!("нет max-original/MAX.RES — тест пропущен");
        return;
    };
    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия стартует");

    let first = app.autosave().expect("первый автосейв");
    assert_eq!(first, dir.join("save10.dta"));
    let first_bytes = std::fs::read(&first).expect("файл первого автосейва");
    assert!(
        !dir.join("save10.bak").exists(),
        "у первого автосейва предыдущего слота нет"
    );

    // Advance the match: the second autosave has to overwrite the first.
    let turn = app.state_ref().turn_counter();
    app.universe_mut().set_turn_counter(turn + 1);
    let second = app.autosave().expect("второй автосейв");
    let second_bytes = std::fs::read(&second).expect("файл второго автосейва");
    assert_ne!(first_bytes, second_bytes, "слот перезаписан");

    let backup = dir.join("save10.bak");
    assert!(backup.is_file(), "SaveLoadMenu_CreateBackup создал .bak");
    assert_eq!(
        std::fs::read(&backup).expect("файл .bak"),
        first_bytes,
        ".bak содержит предыдущий слот"
    );
    assert!(!dir.join("save10.ba1").exists(), "ротации .bak.1 нет");
    assert!(!dir.join("save10.bak.1").exists(), "ротации .bak.1 нет");

    std::fs::remove_dir_all(dir).ok();
}

/// A regular (menu/quick) save passes `backup = false` in C++ and creates no
/// `.bak` — only the autosave does.
#[test]
fn regular_save_does_not_create_a_backup() {
    let dir = temp_dir("no_backup");
    let Some(mut app) = local_app(&dir) else {
        eprintln!("нет max-original/MAX.RES — тест пропущен");
        return;
    };
    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия стартует");

    app.save_game_to_slot(2, "First").expect("сейв");
    app.save_game_to_slot(2, "Second").expect("перезапись");

    assert!(!dir.join("save2.bak").exists());
    let save =
        SaveFile::parse(&std::fs::read(dir.join("save2.dta")).expect("файл")).expect("парсится");
    assert_eq!(save.header.save_name(), "Second");

    std::fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------------------
// S1.3 — quick save/load into the last slot
// ---------------------------------------------------------------------------

/// `Alt+S` with an existing last slot asks for confirmation (`OKCancelMenu`) and
/// writes the very same bytes as the window save.
#[test]
fn quick_save_writes_the_last_slot_in_the_window_format() {
    let dir = temp_dir("quick_save");
    let Some(mut app) = local_app(&dir) else {
        eprintln!("нет max-original/MAX.RES — тест пропущен");
        return;
    };
    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия стартует");

    // The window save (what `Save` in the menu does).
    app.save_game_to_slot(3, "Quick save").expect("сейв окна");
    app.set_last_save_slot(3);
    let window_bytes = std::fs::read(dir.join("save3.dta")).expect("файл");

    // `Alt+S` → `OKCancelMenu` with the slot's existing name.
    max_app::ui::handle_session_input(&mut app, InputEvent::Key(Key::Alt('s')));
    let confirm = app.save_confirm().expect("окно подтверждения открыто");
    assert_eq!(
        confirm.action(),
        &SaveConfirmAction::Save {
            slot: 3,
            name: "Quick save".to_string()
        }
    );

    // `OK` (`Enter`) writes the slot again without the full window.
    max_app::ui::handle_session_input(&mut app, InputEvent::Key(Key::Enter));
    assert!(app.save_confirm().is_none(), "подтверждение закрыто");
    assert!(app.save_load().is_none(), "полное окно не открывалось");
    let quick_bytes = std::fs::read(dir.join("save3.dta")).expect("файл");
    assert_eq!(
        window_bytes, quick_bytes,
        "quick save и обычный save дают одни и те же байты"
    );
    assert!(!dir.join("save3.bak").exists(), "quick save не делает .bak");

    // `Esc` cancels the confirmation and does not touch the slot.
    max_app::ui::handle_session_input(&mut app, InputEvent::Key(Key::Alt('s')));
    assert!(app.save_confirm().is_some());
    max_app::ui::handle_session_input(&mut app, InputEvent::Key(Key::Escape));
    assert!(app.save_confirm().is_none());
    assert_eq!(
        std::fs::read(dir.join("save3.dta")).expect("файл"),
        quick_bytes
    );

    std::fs::remove_dir_all(dir).ok();
}

/// With no file in the last slot the quick save falls back to the full window
/// (C++ `GameManager_MenuClickFileButton`).
#[test]
fn quick_save_opens_the_window_for_an_empty_last_slot() {
    let dir = temp_dir("quick_empty");
    let Some(mut app) = local_app(&dir) else {
        eprintln!("нет max-original/MAX.RES — тест пропущен");
        return;
    };
    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия стартует");
    app.set_last_save_slot(7);

    max_app::ui::handle_session_input(&mut app, InputEvent::Key(Key::Alt('s')));
    assert!(app.save_confirm().is_none());
    assert!(
        app.save_load().is_some(),
        "открылось полное окно сохранения"
    );

    std::fs::remove_dir_all(dir).ok();
}

/// `Alt+L` quick loads the last slot after the confirmation.
#[test]
fn quick_load_loads_the_last_slot() {
    let dir = temp_dir("quick_load");
    let Some(mut app) = local_app(&dir) else {
        eprintln!("нет max-original/MAX.RES — тест пропущен");
        return;
    };
    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия стартует");
    app.save_game_to_slot(4, "Quick load").expect("сейв");
    let saved_turn = app.state_ref().turn_counter();
    app.set_last_save_slot(4);

    max_app::ui::handle_session_input(&mut app, InputEvent::Key(Key::Alt('l')));
    let confirm = app.save_confirm().expect("подтверждение загрузки");
    assert_eq!(confirm.action(), &SaveConfirmAction::Load { slot: 4 });
    max_app::ui::handle_session_input(&mut app, InputEvent::Key(Key::Enter));

    assert!(app.save_confirm().is_none());
    assert_eq!(app.state_ref().turn_counter(), saved_turn);
    assert_eq!(
        app.last_save_slot(),
        4,
        "загруженный слот становится целью quick save"
    );

    std::fs::remove_dir_all(dir).ok();
}

// ---------------------------------------------------------------------------
// S1.4 — `FilePathGamePref` vs `FilePathGameData`
// ---------------------------------------------------------------------------

/// Custom/hot-seat saves go to the preference directory, campaign saves go to the
/// game data directory (`SaveLoad_GetFilePath`, `saveload.cpp:72-78`).
#[test]
fn save_directory_follows_the_mission_category() {
    let pref = temp_dir("split_pref");
    let data = temp_dir("split_data");
    let Some(mut app) = local_app_split(&pref, Some(&data)) else {
        eprintln!("нет max-original/MAX.RES — тест пропущен");
        return;
    };
    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия стартует");

    // Custom → `FilePathGamePref`.
    let custom = app.save_game_to_slot(1, "Custom").expect("custom save");
    assert_eq!(custom, pref.join("save1.dta"));
    assert!(pref.join("save1.dta").is_file());
    assert!(!data.join("save1.dta").exists());

    // The load path of the same category looks in the preference directory.
    app.load_game_slot(1).expect("custom load");

    // Campaign → `FilePathGameData`.
    app.universe_mut()
        .set_mission_category(MissionCategory::Campaign);
    let campaign = app.save_game_to_slot(1, "Campaign").expect("campaign save");
    assert_eq!(campaign, data.join("save1.cam"));
    assert!(data.join("save1.cam").is_file());
    assert!(!pref.join("save1.cam").exists(), "не в pref-каталоге");

    // The load path of a campaign looks in the game data directory.
    app.load_game_slot(1).expect("campaign load");

    std::fs::remove_dir_all(pref).ok();
    std::fs::remove_dir_all(data).ok();
}
