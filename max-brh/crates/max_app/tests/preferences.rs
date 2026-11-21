//! `Preferences`/`Setup`: диалог из меню и из игры, реальные эффекты и звук,
//! сохранение в `IniPreferences` (V70) и `preferences.ini`.
//!
//! Оракул — `max-cpp-port/src/optionsmenu.cpp`, `menu.cpp:2325/2331`,
//! `settings.cpp`, `saveload.cpp:920-934`. Тесты, которым нужны данные игры,
//! пропускаются, если `max-original/MAX.RES` рядом нет.

use std::path::{Path, PathBuf};

use max_app::audio::{AudioPlayer, NoopBackend};
use max_app::local::LocalSetup;
use max_app::preferences::{Preferences, PreferencesAction, PreferencesVariant, PrefKey};
use max_app::qa::runner::repo_root;
use max_app::ui::{InputEvent, Key, MenuClient, Screen, Transition, UiContext};
use max_app::{App, AppConfig, AppMode};
use max_core::enums::{PlayMode as GamePlayMode, UnitOrder, UnitOrderState, UnitType, VictoryType};
use max_core::save::SaveFile;
use max_core::team::{TeamId, TeamKind};
use max_net::{CreateGameParams, GameInfo};
use max_assets::WavClip;

fn repo_path(relative: &str) -> PathBuf {
    repo_root().join(relative)
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "max_app_prefs_{tag}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn app_with_assets(save_dir: &Path) -> Option<App> {
    let data = repo_path("max-original");

    if !data.join("MAX.RES").is_file() {
        eprintln!("preferences: нет max-original/MAX.RES — тест пропущен");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(&data)
        .with_assets_dir(repo_path("assets"))
        .with_mode(AppMode::Ui)
        .with_save_dir(save_dir);
    let mut app = App::new(config);
    app.init_assets().ok()?;
    app.state_mut().teams[0].team_type = TeamKind::Player;
    app.state_mut().team_units[0] = max_core::TeamUnits::with_clan(max_core::TeamClan::Random);
    app.viewport_mut().set_zoom(64);

    Some(app)
}

fn setup_vs_one_ai() -> LocalSetup {
    LocalSetup {
        player_name: "Tester".to_string(),
        world: 5,
        opponent: 2,
        timer: 0,
        endturn: 0,
        play_mode: GamePlayMode::TurnBased,
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

fn changed_pixels(left: &max_render::buffer::RgbaBuffer, right: &max_render::buffer::RgbaBuffer) -> usize {
    left.pixels()
        .chunks_exact(4)
        .zip(right.pixels().chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count()
}

/// Клик по кнопке `Preferences` панели открывает диалог; клик по чекбоксу и
/// `Done` применяют и сохраняют настройку (C++ `GameManager_MenuClickPreferencesButton`
/// → `OptionsMenu(PREFSPIC)`).
#[test]
fn in_game_panel_button_opens_preferences_and_done_saves_effects() {
    let dir = temp_dir("panel");
    let Some(mut app) = app_with_assets(&dir) else {
        return;
    };

    let button = max_app::panels::PanelButton::Preferences.rect();
    let transition = max_app::ui::handle_session_input(
        &mut app,
        InputEvent::MouseClick(button.ulx + button.width / 2, button.uly + button.height / 2),
        false,
    );
    assert_eq!(transition, Transition::None);
    assert!(app.preferences_dialog().is_some(), "диалог Preferences открыт");

    // Пока диалог открыт, панель/карта ввода не получают: клик по карте ничего
    // не выделяет (модальность `OptionsMenu::Run`).
    let selected_before = app.hud().selected;
    max_app::ui::handle_session_input(&mut app, InputEvent::MouseClick(404, 242), false);
    assert_eq!(app.hud().selected, selected_before);

    let effects = app
        .preferences_dialog()
        .expect("dialog")
        .screen_rect(PrefKey::Effects)
        .expect("effects checkbox");
    let action = app
        .preferences_input(InputEvent::MouseClick(
            effects.ulx + effects.width / 2,
            effects.uly + effects.height / 2,
        ))
        .expect("dialog");
    assert_eq!(action, PreferencesAction::Changed);
    assert!(!app.effects_enabled(), "Animate Effects выключен сразу");

    let done = app
        .preferences_dialog()
        .expect("dialog")
        .screen_rect(PrefKey::Effects)
        .expect("row");
    let _ = done;
    let action = app
        .preferences_input(InputEvent::MouseClick(
            max_app::preferences::WINDOW_ULX + max_app::preferences::DONE_BUTTON.ulx + 31,
            max_app::preferences::WINDOW_ULY + max_app::preferences::DONE_BUTTON.uly + 11,
        ))
        .expect("dialog");
    assert_eq!(action, PreferencesAction::Done);
    assert!(app.preferences_dialog().is_none(), "Done закрывает диалог");
    assert!(!app.effects_enabled(), "Done сохраняет значение");

    let path = app.preferences_path();
    let text = std::fs::read_to_string(&path).expect("preferences.ini записан");
    assert!(text.contains("effects=0"), "{text}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `effects = false` останавливает покадровые анимации: `RADAR` (`ANIMATED`)
/// не листает кадры, кадр статичен (`settings["effects"]` в C++).
#[test]
fn effects_off_freezes_building_animation_frames() {
    let dir = temp_dir("effects");
    let Some(mut app) = app_with_assets(&dir) else {
        return;
    };

    let radar = app
        .state_mut()
        .spawn_unit(UnitType::Radar, TeamId::RED, max_core::geo::Point::new(40, 40), 0)
        .expect("radar");
    app.viewport_mut().center_on_grid(40, 40);
    {
        let unit = app.state_mut().unit_mut(radar).expect("radar");
        unit.orders = UnitOrder::Sentry;
        unit.state = UnitOrderState::ExecutingOrder;
    }

    let static_frame = app.render_frame();
    app.update_visual_units(1.0 / 30.0 + 0.001);
    assert_eq!(app.unit_animation(radar).sentry_step, 1);
    assert!(
        changed_pixels(&static_frame, &app.render_frame()) > 0,
        "effects = on: ANIMATED-здание листает кадр"
    );

    // Выключаем эффекты — часы сбрасываются, кадры больше не идут.
    app.set_preferences(Preferences {
        effects: false,
        ..app.preferences().clone()
    });
    app.update_visual_units(0.0);
    let off_frame = app.render_frame();

    for _ in 0..5 {
        app.update_visual_units(1.0 / 30.0 + 0.001);
        assert_eq!(
            app.unit_animation(radar).sentry_step,
            0,
            "effects = off: кадры не листаются"
        );
        assert_eq!(
            changed_pixels(&off_frame, &app.render_frame()),
            0,
            "effects = off: кадр статичен"
        );
    }

    // Включаем обратно — анимация продолжается.
    app.set_preferences(Preferences {
        effects: true,
        ..app.preferences().clone()
    });
    app.update_visual_units(1.0 / 30.0 + 0.001);
    assert_eq!(app.unit_animation(radar).sentry_step, 1);
    assert!(changed_pixels(&off_frame, &app.render_frame()) > 0);

    let _ = std::fs::remove_dir_all(&dir);
}

/// `FX Disabled` (`disable_fx`) — «звук вкл/выкл»: флаг доходит до
/// `AudioPlayer::set_enabled` и `play` больше не ставит клип в очередь.
#[test]
fn sound_flag_reaches_the_audio_layer() {
    let dir = temp_dir("sound");
    let Some(mut app) = app_with_assets(&dir) else {
        return;
    };

    let mut audio = AudioPlayer::with_backend(Box::new(NoopBackend));
    audio.preload(
        "KBUY0",
        WavClip {
            sample_rate: 22_050,
            channels: 1,
            samples: vec![0; 64],
        },
    );
    audio.set_enabled(app.sound_enabled());
    assert!(audio.enabled());
    assert!(audio.play_once("KBUY0"));
    assert_eq!(audio.plays(), 1);

    // Клик по `FX Disabled` в диалоге (PREFSPIC).
    assert!(app.open_preferences());
    let fx = app
        .preferences_dialog()
        .expect("dialog")
        .screen_rect(PrefKey::DisableFx)
        .expect("fx checkbox");
    app.preferences_input(InputEvent::MouseClick(
        fx.ulx + fx.width / 2,
        fx.uly + fx.height / 2,
    ))
    .expect("dialog");
    assert!(!app.sound_enabled(), "disable_fx = 1");

    // `.runtime`-синхронизация как `main.rs`/QA: флаг → AudioPlayer.
    audio.set_enabled(app.sound_enabled());
    audio.set_volume(app.preferences().sound_volume());
    assert!(!audio.enabled());
    assert!(!audio.play_once("KBUY0"), "звук выключен");
    assert_eq!(audio.plays(), 1, "новых клипов не поставлено");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `IniPreferences` пишутся в настоящий V70-сейв (`SaveLoad_Save`,
/// `saveload.cpp:547`) и читаются обратно (как `SaveLoad_Load` применяет их).
#[test]
fn preferences_are_written_to_and_read_from_v70_ini_preferences() {
    let dir = temp_dir("v70");
    let Some(mut app) = app_with_assets(&dir) else {
        return;
    };
    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия");

    let mut prefs = Preferences::default();
    prefs.effects = false;
    prefs.click_scroll = false;
    prefs.quick_scroll = 40;
    prefs.fast_movement = false;
    prefs.follow_unit = true;
    prefs.auto_select = true;
    prefs.enemy_halt = false;
    app.set_preferences(prefs.clone());

    let path = dir.join("prefs.dta");
    app.save_v70_to(&path, "Preferences").expect("V70");

    let save = SaveFile::parse(&std::fs::read(&path).expect("файл")).expect("parses");
    assert_eq!(save.ini_preferences, prefs.to_ini());

    // Загрузка применяет ini_preferences к настройкам (C++ `SaveLoad_Load`).
    let mut fresh = app_with_assets(&dir).expect("assets");
    fresh.load_v70_path(&path).expect("load");
    assert_eq!(fresh.preferences().to_ini(), prefs.to_ini());
    assert!(!fresh.effects_enabled());

    let _ = std::fs::remove_dir_all(&dir);
}

/// Пункт `Setup` главного меню открывает `OptionsMenu(SETUPPIC)`; `Done`
/// отдаёт значения переходом `ApplyPreferences` (`menu_setup_window`).
#[test]
fn main_menu_setup_opens_dialog_and_done_applies_preferences() {
    struct DummyClient;

    impl MenuClient for DummyClient {
        fn list_games(&mut self) -> Result<Vec<GameInfo>, String> {
            Ok(Vec::new())
        }

        fn create_game(&mut self, _params: CreateGameParams) -> Result<GameInfo, String> {
            Err("not used".to_string())
        }
    }

    let mut screen = Screen::MainMenu;
    let mut ctx = UiContext::new(Box::new(DummyClient), 640, 480);
    let button = max_app::menu::MAIN_MENU_BUTTONS[3].rect;

    let transition = screen.handle_input(
        &mut ctx,
        InputEvent::MouseClick(
            button.ulx + button.width / 2,
            button.uly + button.height / 2,
        ),
    );
    assert_eq!(transition, Transition::None);
    let Screen::Preferences(dialog) = &screen else {
        panic!("Setup должен открыть Screen::Preferences, сейчас {screen:?}");
    };
    assert_eq!(dialog.variant(), PreferencesVariant::Setup);
    // `SETUPPIC` заканчивается на секции `preferences`.
    assert!(dialog.row(PrefKey::Effects).is_none());
    assert!(dialog.row(PrefKey::DisableFx).is_some());

    // `FX Disabled` виден и переключается: `Changed` обновляет `ctx`.
    let fx = dialog.screen_rect(PrefKey::DisableFx).expect("fx");
    let transition = screen.handle_input(
        &mut ctx,
        InputEvent::MouseClick(fx.ulx + fx.width / 2, fx.uly + fx.height / 2),
    );
    assert_eq!(transition, Transition::None);
    assert!(ctx.preferences.disable_fx, "Changed обновляет контекст");

    // `Enter` — Done: переход с готовыми настройками и возврат в меню.
    let transition = screen.handle_input(&mut ctx, InputEvent::Key(Key::Enter));
    match transition {
        Transition::ApplyPreferences(prefs) => {
            assert!(prefs.disable_fx);
        }
        other => panic!("ожидался ApplyPreferences, получено {other:?}"),
    }
    assert!(matches!(screen, Screen::MainMenu));
}

/// `Esc`/`Cancel` возвращает `Cancel`, а вызывающий откатывает снимок
/// (`last_rest_state` в C++).
#[test]
fn cancel_reverts_the_draft() {
    let mut dialog = max_app::preferences::PreferencesDialog::new(
        PreferencesVariant::InGame,
        Preferences::default(),
        max_app::preferences::PreferencesInfo::default(),
    );
    let effects = dialog.screen_rect(PrefKey::Effects).expect("effects");
    dialog.input(InputEvent::MouseClick(
        effects.ulx + effects.width / 2,
        effects.uly + effects.height / 2,
    ));
    assert!(!dialog.draft.effects);

    assert_eq!(
        dialog.input(InputEvent::Key(Key::Escape)),
        PreferencesAction::Cancel
    );
    // Откат делает приложение по `initial` — здесь проверяем сам снимок.
    assert!(dialog.initial.effects, "initial не меняется");
}
