//! `Preferences`/`Setup`: the dialog from the menu and from the game, real effects and sound,
//! saving to `IniPreferences` (V70) and `preferences.ini`.
//!
//! Oracle — `max-cpp-port/src/optionsmenu.cpp`, `menu.cpp:2325/2331`,
//! `settings.cpp`, `saveload.cpp:920-934`. Tests that need game data are
//! skipped if there is no `max-original/MAX.RES` nearby.

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
    app.universe_mut().teams_mut()[0].team_type = TeamKind::Player;
    app.universe_mut().team_units_table_mut()[0] = max_core::TeamUnits::with_clan(max_core::TeamClan::Random);
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

/// A click on the panel `Preferences` button opens the dialog; a click on a checkbox and
/// `Done` apply and save the setting (C++ `GameManager_MenuClickPreferencesButton`
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
        InputEvent::MouseClick(button.ulx + button.width / 2, button.uly + button.height / 2)
    );
    assert_eq!(transition, Transition::None);
    assert!(app.preferences_dialog().is_some(), "диалог Preferences открыт");

    // While the dialog is open, the panel/map receive no input: a click on the map selects
    // nothing (modality of `OptionsMenu::Run`).
    let selected_before = app.hud().selected;
    max_app::ui::handle_session_input(&mut app, InputEvent::MouseClick(404, 242));
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

/// `effects = false` stops frame-by-frame animations: `RADAR` (`ANIMATED`)
/// does not advance frames, the frame is static (`settings["effects"]` in C++).
#[test]
fn effects_off_freezes_building_animation_frames() {
    let dir = temp_dir("effects");
    let Some(mut app) = app_with_assets(&dir) else {
        return;
    };

    let radar = app
        .universe_mut()
        .spawn_unit(UnitType::Radar, TeamId::RED, max_core::geo::Point::new(40, 40), 0)
        .expect("radar");
    app.viewport_mut().center_on_grid(40, 40);
    {
        let unit = app.universe_mut().unit_mut(radar).expect("radar");
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

    // Disable the effects — the clock resets, frames no longer advance.
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

    // Enable them back — the animation continues.
    app.set_preferences(Preferences {
        effects: true,
        ..app.preferences().clone()
    });
    app.update_visual_units(1.0 / 30.0 + 0.001);
    assert_eq!(app.unit_animation(radar).sentry_step, 1);
    assert!(changed_pixels(&off_frame, &app.render_frame()) > 0);

    let _ = std::fs::remove_dir_all(&dir);
}

/// `FX Disabled` (`disable_fx`) — "sound on/off": the flag reaches
/// `AudioPlayer::set_enabled` and `play` no longer queues the clip.
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

    // A click on `FX Disabled` in the dialog (PREFSPIC).
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

    // `.runtime` synchronization like `main.rs`/QA: flag → AudioPlayer.
    audio.set_enabled(app.sound_enabled());
    audio.set_volume(app.preferences().sound_volume());
    assert!(!audio.enabled());
    assert!(!audio.play_once("KBUY0"), "звук выключен");
    assert_eq!(audio.plays(), 1, "новых клипов не поставлено");

    let _ = std::fs::remove_dir_all(&dir);
}

/// `IniPreferences` are written into a real V70 save (`SaveLoad_Save`,
/// `saveload.cpp:547`) and read back (as `SaveLoad_Load` applies them).
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

    // Loading applies ini_preferences to the settings (C++ `SaveLoad_Load`).
    let mut fresh = app_with_assets(&dir).expect("assets");
    fresh.load_v70_path(&path).expect("load");
    assert_eq!(fresh.preferences().to_ini(), prefs.to_ini());
    assert!(!fresh.effects_enabled());

    let _ = std::fs::remove_dir_all(&dir);
}

/// The main menu `Setup` item opens `OptionsMenu(SETUPPIC)`; `Done`
/// returns the values via the `ApplyPreferences` transition (`menu_setup_window`).
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
    // `SETUPPIC` ends at the `preferences` section.
    assert!(dialog.row(PrefKey::Effects).is_none());
    assert!(dialog.row(PrefKey::DisableFx).is_some());

    // `FX Disabled` is visible and toggles: `Changed` updates `ctx`.
    let fx = dialog.screen_rect(PrefKey::DisableFx).expect("fx");
    let transition = screen.handle_input(
        &mut ctx,
        InputEvent::MouseClick(fx.ulx + fx.width / 2, fx.uly + fx.height / 2),
    );
    assert_eq!(transition, Transition::None);
    assert!(ctx.preferences.disable_fx, "Changed обновляет контекст");

    // `Enter` — Done: the transition with the ready settings and return to the menu.
    let transition = screen.handle_input(&mut ctx, InputEvent::Key(Key::Enter));
    match transition {
        Transition::ApplyPreferences(prefs) => {
            assert!(prefs.disable_fx);
        }
        other => panic!("ожидался ApplyPreferences, получено {other:?}"),
    }
    assert!(matches!(screen, Screen::MainMenu));
}

/// `Esc`/`Cancel` returns `Cancel`, and the caller rolls back the snapshot
/// (`last_rest_state` in C++).
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
    // The application does the rollback via `initial` — here we check the snapshot itself.
    assert!(dialog.initial.effects, "initial не меняется");
}
