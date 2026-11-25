//! `Preferences`/`Setup`: the dialog from the menu and from the game, real effects and sound,
//! saving to `IniPreferences` (V70) and `preferences.ini`.
//!
//! Oracle — `max-cpp-port/src/optionsmenu.cpp`, `menu.cpp:2325/2331`,
//! `settings.cpp`, `saveload.cpp:920-934`. Tests that need game data are
//! require `max-original/MAX.RES` and fail without it.

use std::path::{Path, PathBuf};

use max_app::audio::{AudioPlayer, NoopBackend};
use max_app::local::LocalSetup;
use max_app::preferences::{
    Preferences, PreferencesAction, PreferencesInfo, PreferencesVariant, PrefKey,
    DEBUG_SECTION_BUILT,
};
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
        .with_assets_dir(repo_path("tests/fixtures/assets"))
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
    let mut app = app_with_assets(&dir).expect(concat!("test fixture is missing: ", stringify!(app_with_assets(&dir))));

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
    let mut app = app_with_assets(&dir).expect(concat!("test fixture is missing: ", stringify!(app_with_assets(&dir))));

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
    let mut app = app_with_assets(&dir).expect(concat!("test fixture is missing: ", stringify!(app_with_assets(&dir))));

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
    let mut app = app_with_assets(&dir).expect(concat!("test fixture is missing: ", stringify!(app_with_assets(&dir))));
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

// ---------------------------------------------------------------------------
// B6 — debug section (`invalid`)
// ---------------------------------------------------------------------------
//
// Oracle: `optionsmenu.hpp` (controls 30..35) and
// `GameManager_MenuClickPreferencesButton` (`game_manager.cpp:3925-3943`). The
// section is a developer tool: it is built in debug builds only and needs
// `settings["debug"]`, so every test below returns early in a release build.

/// Application without game data: the debug section only needs the facade.
fn plain_app(save_dir: &Path) -> App {
    App::new(AppConfig::default().with_save_dir(save_dir))
}

/// A click on `Disable Fire` changes the settings draft at once, but the match is
/// written only when the dialog closes — through the facade install API, never by
/// the UI (rule C3). `Done` transfers both switches (`real_time` too), like
/// `GameManager_MenuClickPreferencesButton` re-reading `settings`.
#[cfg(debug_assertions)]
#[test]
fn debug_switches_install_through_the_facade_when_the_dialog_closes() {

    let dir = temp_dir("debug-install");
    let mut app = plain_app(&dir);
    app.set_preferences(Preferences {
        debug: true,
        ..Preferences::default()
    });

    assert!(app.open_preferences());
    let dialog = app.preferences_dialog().expect("dialog");
    assert!(dialog.has_debug_section(), "debug = 1 opens the section");
    let disable_fire = dialog
        .screen_rect(PrefKey::DisableFire)
        .expect("Disable Fire");
    let real_time = dialog.screen_rect(PrefKey::RealTime).expect("Real Time");

    assert_eq!(
        app.preferences_input(InputEvent::MouseClick(
            disable_fire.ulx + disable_fire.width / 2,
            disable_fire.uly + disable_fire.height / 2,
        )),
        Some(PreferencesAction::Changed)
    );
    assert!(app.preferences().disable_fire, "the draft changed");
    assert!(
        !app.state_ref().disable_fire(),
        "C3: a toggle never writes GameState directly"
    );

    assert_eq!(
        app.preferences_input(InputEvent::MouseClick(
            real_time.ulx + real_time.width / 2,
            real_time.uly + real_time.height / 2,
        )),
        Some(PreferencesAction::Changed)
    );
    assert!(!app.state_ref().real_time());

    // `Done` closes the window: the settings are installed into the match.
    assert_eq!(
        app.preferences_input(InputEvent::Key(Key::Enter)),
        Some(PreferencesAction::Done)
    );
    assert!(app.preferences_dialog().is_none());
    assert!(app.state_ref().disable_fire(), "installed on Done");
    assert!(app.state_ref().real_time(), "installed on Done");

    let _ = std::fs::remove_dir_all(&dir);
}

/// The four team fields are `EDIT_INT` controls (`red_team_player` …): the value
/// reaches the settings on `Done` and the live `TeamInfo::team_type` through
/// `GameUniverse::set_team_type` — the same slot C++ writes in
/// `GameManager_MenuClickPreferencesButton`.
#[cfg(debug_assertions)]
#[test]
fn debug_team_field_installs_the_team_control_slot() {

    let dir = temp_dir("debug-team");
    let mut app = plain_app(&dir);
    app.set_preferences(Preferences {
        debug: true,
        ..Preferences::default()
    });
    assert!(app.open_preferences());

    // An empty world starts with every slot empty; the dialog shows what is installed.
    assert_eq!(
        app.preferences_dialog()
            .expect("dialog")
            .value(PrefKey::RedTeamPlayer),
        Some(TeamKind::None as i32)
    );

    let rect = app
        .preferences_dialog()
        .expect("dialog")
        .screen_rect(PrefKey::RedTeamPlayer)
        .expect("Red Team");
    let (x, y) = (rect.ulx + rect.width / 2, rect.uly + rect.height / 2);

    assert_eq!(
        app.preferences_input(InputEvent::MouseClick(x, y)),
        Some(PreferencesAction::None)
    );
    // "0" -> backspace -> "3" -> Enter commits the field (`TextEdit`, `EDIT_INT`).
    assert_eq!(
        app.preferences_input(InputEvent::Key(Key::Backspace)),
        Some(PreferencesAction::None)
    );
    assert_eq!(
        app.preferences_input(InputEvent::Key(Key::Char('3'))),
        Some(PreferencesAction::None)
    );
    assert_eq!(
        app.preferences_input(InputEvent::Key(Key::Enter)),
        Some(PreferencesAction::Changed)
    );

    // `EDIT_INT` reaches the settings only on `Done` (`optionsmenu.cpp:533-560`).
    assert_eq!(app.preferences().team_players[0], TeamKind::None);
    assert_eq!(
        app.state_ref().team(TeamId::RED).expect("red").team_type,
        TeamKind::None
    );

    assert_eq!(
        app.preferences_input(InputEvent::Key(Key::Enter)),
        Some(PreferencesAction::Done)
    );
    assert_eq!(
        app.state_ref().team(TeamId::RED).expect("red").team_type,
        TeamKind::Remote,
        "the installed slot is `TEAM_TYPE_REMOTE` (3)"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The digest gate: `debug` is UI state and leaves `snapshot_bytes` untouched; the
/// switches it reveals are simulation fields (`game.rs:357/360`), so installing one
/// **does** move the digest — by design, C++ sends both in the match setup packet
/// (`remote.cpp:2469-2471`). While the dialog is open nothing is installed.
#[cfg(debug_assertions)]
#[test]
fn debug_gate_keeps_the_snapshot_and_the_installed_switch_moves_it() {

    let dir = temp_dir("debug-digest");
    let mut app = plain_app(&dir);
    let before = app.state_ref().snapshot_bytes();

    // Gate off: open and close the dialog — the canonical bytes do not move.
    assert!(app.open_preferences());
    assert!(!app.preferences_dialog().expect("dialog").has_debug_section());
    assert_eq!(
        app.preferences_input(InputEvent::Key(Key::Enter)),
        Some(PreferencesAction::Done)
    );
    assert_eq!(
        app.state_ref().snapshot_bytes(),
        before,
        "`debug` is UI state, outside the snapshot"
    );

    // Gate on: the draft changes, the digest still does not move until `Done`.
    app.set_preferences(Preferences {
        debug: true,
        ..app.preferences().clone()
    });
    let before = app.state_ref().snapshot_bytes();
    assert!(app.open_preferences());
    let rect = app
        .preferences_dialog()
        .expect("dialog")
        .screen_rect(PrefKey::DisableFire)
        .expect("Disable Fire");
    app.preferences_input(InputEvent::MouseClick(
        rect.ulx + rect.width / 2,
        rect.uly + rect.height / 2,
    ));
    assert_eq!(
        app.state_ref().snapshot_bytes(),
        before,
        "the dialog only edits the draft"
    );

    app.preferences_input(InputEvent::Key(Key::Enter));
    assert_ne!(
        app.state_ref().snapshot_bytes(),
        before,
        "`disable_fire` is a simulation field of the digest (`game.rs:357`)"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `Cancel` restores the values the dialog opened with and re-installs them, so the
/// match keeps running with the settings it had (`last_rest_state` in C++).
#[cfg(debug_assertions)]
#[test]
fn debug_cancel_restores_the_installed_values() {

    let dir = temp_dir("debug-cancel");
    let mut app = plain_app(&dir);
    app.set_preferences(Preferences {
        debug: true,
        ..Preferences::default()
    });

    // Install `Disable Fire = 1` with `Done`.
    assert!(app.open_preferences());
    let rect = app
        .preferences_dialog()
        .expect("dialog")
        .screen_rect(PrefKey::DisableFire)
        .expect("Disable Fire");
    app.preferences_input(InputEvent::MouseClick(
        rect.ulx + rect.width / 2,
        rect.uly + rect.height / 2,
    ));
    assert_eq!(
        app.preferences_input(InputEvent::Key(Key::Enter)),
        Some(PreferencesAction::Done)
    );
    assert!(app.state_ref().disable_fire());

    // Reopen: the dialog shows the installed value. Toggle it off and cancel.
    assert!(app.open_preferences());
    assert_eq!(
        app.preferences_dialog()
            .expect("dialog")
            .value(PrefKey::DisableFire),
        Some(1)
    );
    let rect = app
        .preferences_dialog()
        .expect("dialog")
        .screen_rect(PrefKey::DisableFire)
        .expect("Disable Fire");
    app.preferences_input(InputEvent::MouseClick(
        rect.ulx + rect.width / 2,
        rect.uly + rect.height / 2,
    ));
    assert!(
        app.state_ref().disable_fire(),
        "still only the draft while the dialog is open"
    );
    assert_eq!(
        app.preferences_input(InputEvent::Key(Key::Escape)),
        Some(PreferencesAction::Cancel)
    );
    assert!(
        app.state_ref().disable_fire(),
        "Cancel puts the opening values back"
    );
    assert!(app.preferences().disable_fire);

    let _ = std::fs::remove_dir_all(&dir);
}

/// The panel itself: the six debug rows are painted over `PREFSPIC` above the
/// dialog buttons, and `debug = 0` paints the old frame.
#[cfg(debug_assertions)]
#[test]
fn debug_section_is_painted_above_the_dialog_buttons() {

    let dir = temp_dir("debug-draw");
    let app = app_with_assets(&dir).expect(concat!("test fixture is missing: ", stringify!(app_with_assets(&dir))));
    let assets = app.assets().expect("assets");
    let info = PreferencesInfo::default();
    let off = max_app::preferences::PreferencesDialog::new(
        PreferencesVariant::InGame,
        Preferences::default(),
        info,
    );
    let on = max_app::preferences::PreferencesDialog::new(
        PreferencesVariant::InGame,
        Preferences {
            debug: true,
            ..Preferences::default()
        },
        info,
    );

    let mut plain = max_render::buffer::RgbaBuffer::new(640, 480);
    let mut debug = max_render::buffer::RgbaBuffer::new(640, 480);
    assert!(off.draw(&mut plain, assets) > 0, "the dialog paints");
    assert!(on.draw(&mut debug, assets) > 0, "the debug dialog paints");
    assert!(
        changed_pixels(&plain, &debug) > 0,
        "the debug section draws six more rows"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
