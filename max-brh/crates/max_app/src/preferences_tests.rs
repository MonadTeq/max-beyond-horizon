use super::*;

fn dialog(variant: PreferencesVariant) -> PreferencesDialog {
    PreferencesDialog::new(variant, Preferences::default(), PreferencesInfo::default())
}

/// `OptionsMenu::Init` layout for `PREFSPIC` (without the debug section).
///
/// `window_uly` = 20; section +20; each option with `ulx == 25` +20; label
/// after drawing `−7` (`20 − (10 + 3)`).
#[test]
fn in_game_layout_matches_the_cpp_window_uly_algorithm() {
    let dialog = dialog(PreferencesVariant::InGame);
    let y = |key| dialog.row(key).map(|row| row.y);

    assert_eq!(y(PrefKey::MusicVolumeLabel), Some(60));
    assert_eq!(y(PrefKey::MusicLevel), Some(73));
    assert_eq!(y(PrefKey::DisableMusic), Some(73));
    assert_eq!(y(PrefKey::FxSoundLevel), Some(93));
    assert_eq!(y(PrefKey::DisableFx), Some(93));
    assert_eq!(y(PrefKey::VoiceLevel), Some(113));
    assert_eq!(y(PrefKey::MovieLevel), Some(133));
    assert_eq!(y(PrefKey::AutoSave), Some(153));
    assert_eq!(y(PrefKey::PlayerName), Some(173));
    assert_eq!(y(PrefKey::Effects), Some(213));
    assert_eq!(y(PrefKey::ClickScroll), Some(213));
    assert_eq!(y(PrefKey::FastMovement), Some(233));
    assert_eq!(y(PrefKey::EnemyHalt), Some(253));
    assert_eq!(y(PrefKey::QuickScroll), Some(273));
    assert_eq!(y(PrefKey::TurnTimer), Some(313));
    assert_eq!(y(PrefKey::EndTurn), Some(313));
    assert_eq!(y(PrefKey::PlayModeLabel), Some(333));
    assert_eq!(y(PrefKey::OpponentLabel), Some(346));
    assert_eq!(y(PrefKey::VictoryLabel), Some(359));

    // `enhanced_graphics`/`host_address`/`host_port` — only `SETUPPIC`.
    assert!(dialog.row(PrefKey::EnhancedGraphics).is_none());
    assert!(dialog.row(PrefKey::HostAddress).is_none());
    assert!(dialog.row(PrefKey::HostPort).is_none());
    assert_eq!(dialog.rows().len(), 23);
}

/// `SETUPPIC`: `Init` ends at the `preferences` section.
#[test]
fn setup_layout_stops_at_the_preferences_section() {
    let dialog = dialog(PreferencesVariant::Setup);
    let y = |key| dialog.row(key).map(|row| row.y);

    assert_eq!(y(PrefKey::MusicVolumeLabel), Some(60));
    // Label subtracts 7: the checkbox on the right is drawn at 53.
    assert_eq!(y(PrefKey::EnhancedGraphics), Some(53));
    assert_eq!(y(PrefKey::HostAddress), Some(173));
    assert_eq!(y(PrefKey::HostPort), Some(193));
    assert_eq!(y(PrefKey::PlayerName), Some(213));
    assert!(dialog.row(PrefKey::Effects).is_none());
    assert_eq!(dialog.rows().len(), 14);
}

/// The debug section (`invalid`) follows the C++ `Init` exactly: the in-game
/// list starts at `-20`, the three read-only `options` labels are dropped and the
/// six debug controls land at 313/313/333/333/353/353 — above the 383 button row.
#[cfg(debug_assertions)]
#[test]
fn debug_layout_matches_the_cpp_window_uly_algorithm() {

    let prefs = Preferences {
        debug: true,
        ..Preferences::default()
    };
    let dialog = PreferencesDialog::new(
        PreferencesVariant::InGame,
        prefs,
        PreferencesInfo::default(),
    );
    let y = |key| dialog.row(key).map(|row| row.y);

    assert!(dialog.has_debug_section());

    // `window_uly = -20`: the whole shared block is 40 px higher than usual.
    assert_eq!(y(PrefKey::MusicVolumeLabel), Some(20));
    assert_eq!(y(PrefKey::TurnTimer), Some(273));
    // The three labels the debug section replaces.
    assert!(dialog.row(PrefKey::PlayModeLabel).is_none());
    assert!(dialog.row(PrefKey::OpponentLabel).is_none());
    assert!(dialog.row(PrefKey::VictoryLabel).is_none());
    // Six debug controls: `disable_fire`/`real_time`, then four team fields.
    assert_eq!(y(PrefKey::DisableFire), Some(313));
    assert_eq!(y(PrefKey::RealTime), Some(313));
    assert_eq!(y(PrefKey::RedTeamPlayer), Some(333));
    assert_eq!(y(PrefKey::GreenTeamPlayer), Some(333));
    assert_eq!(y(PrefKey::BlueTeamPlayer), Some(353));
    assert_eq!(y(PrefKey::GrayTeamPlayer), Some(353));
    assert_eq!(dialog.rows().len(), 26);

    let last = dialog.rows().last().expect("debug row");
    assert!(
        last.y + 20 <= 383,
            "debug section ends above the dialog buttons: y = {}",
        last.y
    );

    // `SETUPPIC` still stops at `preferences`: no debug controls in the menu.
    let setup = PreferencesDialog::new(
        PreferencesVariant::Setup,
        Preferences {
            debug: true,
            ..Preferences::default()
        },
        PreferencesInfo::default(),
    );
    assert!(setup.rows().iter().all(|row| row.key.team_slot().is_none()));
    assert_eq!(setup.rows().len(), 14);
}

/// The gate needs both a debug build (`#if !defined(NDEBUG)`) and `settings["debug"]`.
#[test]
fn debug_section_is_gated_by_the_build_and_the_setting() {
    let enabled = Preferences {
        debug: true,
        ..Preferences::default()
    };

    assert_eq!(debug_section_enabled(&enabled), DEBUG_SECTION_BUILT);
    assert!(!debug_section_enabled(&Preferences::default()));
    assert!(
        !dialog(PreferencesVariant::InGame).has_debug_section(),
            "`debug = 0` (settings.cpp default) keeps the section out"
    );
}

/// The QA/`preferences.ini` names of the debug controls and their C++ ranges.
#[test]
fn debug_keys_parse_and_carry_the_cpp_ranges() {
    for (key, name) in [
        (PrefKey::DisableFire, "disable_fire"),
        (PrefKey::RealTime, "real_time"),
        (PrefKey::RedTeamPlayer, "red_team_player"),
        (PrefKey::GreenTeamPlayer, "green_team_player"),
        (PrefKey::BlueTeamPlayer, "blue_team_player"),
        (PrefKey::GrayTeamPlayer, "gray_team_player"),
    ] {
        assert_eq!(key.name(), name);
        assert_eq!(PrefKey::parse(name), Some(key));
    }

    assert_eq!(PrefKey::parse("red_team"), Some(PrefKey::RedTeamPlayer));
    assert!(PrefKey::DisableFire.is_checkbox());
    assert!(PrefKey::RealTime.is_checkbox());
    assert!(!PrefKey::DisableFire.is_edit());
    assert!(PrefKey::RedTeamPlayer.is_edit());
    assert!(!PrefKey::RedTeamPlayer.is_checkbox());
    assert_eq!(PrefKey::range(PrefKey::RedTeamPlayer), (0, 3));
    assert_eq!(PrefKey::range(PrefKey::GrayTeamPlayer), (0, 3));
    assert_eq!(PrefKey::RedTeamPlayer.team_slot(), Some(0));
    assert_eq!(PrefKey::GrayTeamPlayer.team_slot(), Some(3));
    assert_eq!(PrefKey::RealTime.team_slot(), None);
}

/// The team slots are `0..=3` (`TEAM_TYPE_NONE..=TEAM_TYPE_REMOTE`) and clamp.
#[test]
fn team_slots_roundtrip_and_clamp_like_the_edit_fields() {
    let mut prefs = Preferences::default();
    assert_eq!(
        prefs.team_players,
        [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None
        ]
    );
    assert_eq!(prefs.int_value(PrefKey::RedTeamPlayer), Some(1));
    assert_eq!(prefs.int_value(PrefKey::GreenTeamPlayer), Some(2));

    assert!(prefs.set_int(PrefKey::RedTeamPlayer, 3));
    assert_eq!(prefs.team_players[0], TeamKind::Remote);

    // Clamped: `Eliminated` (4) is not reachable from the dialog.
    assert!(prefs.set_int(PrefKey::BlueTeamPlayer, 9));
    assert_eq!(prefs.team_players[2], TeamKind::Remote);
    assert!(prefs.set_int(PrefKey::BlueTeamPlayer, -1));
    assert_eq!(prefs.team_players[2], TeamKind::None);

    // Checkbox accessors.
    assert!(!prefs.bool_value(PrefKey::DisableFire).unwrap_or(true));
    assert!(prefs.set_bool(PrefKey::DisableFire, true));
    assert!(prefs.disable_fire);
    assert!(prefs.set_bool(PrefKey::RealTime, true));
    assert!(prefs.real_time);
}

/// Settings file: the debug section survives write → read.
#[test]
fn debug_preferences_file_roundtrip() {
    let dir = std::env::temp_dir().join(format!(
            "max-brh-prefs-debug-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or(0)
    ));
    let path = preferences_path_for(&dir);
    let prefs = Preferences {
        debug: true,
        disable_fire: true,
        real_time: true,
        team_players: [
            TeamKind::Remote,
            TeamKind::None,
            TeamKind::Player,
            TeamKind::Computer,
        ],
        ..Preferences::default()
    };

    prefs.save_file(&path).expect("save");
    let loaded = Preferences::load_file(&path).expect("load");
    assert_eq!(loaded, prefs);

    let text = std::fs::read_to_string(&path).expect("file");
    assert!(text.contains("debug=1"), "{text}");
    assert!(text.contains("disable_fire=1"), "{text}");
    assert!(text.contains("red_team_player=3"), "{text}");
    assert!(text.contains("gray_team_player=2"), "{text}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A click on a checkbox toggles the value (like `win_button_down` in C++),
/// `Enter` — `Done`, `Esc` — `Cancel`.
#[test]
fn checkbox_click_toggles_and_enter_is_done() {
    let mut dialog = dialog(PreferencesVariant::InGame);
    let rect = dialog.screen_rect(PrefKey::Effects).expect("effects");
    let center = (rect.ulx + rect.width / 2, rect.uly + rect.height / 2);

    assert!(dialog.draft.effects);
    assert_eq!(
        dialog.input(InputEvent::MouseClick(center.0, center.1)),
        PreferencesAction::Changed
    );
    assert!(!dialog.draft.effects);

    // `Tab`/`Space`: focus starts from the first control (`Volume:` label),
    // the second `Tab` — the `Music` slider; `Space` on a slider does nothing.
    dialog.focus = None;
    assert_eq!(dialog.input(InputEvent::Key(Key::Tab)), PreferencesAction::None);
    assert_eq!(dialog.input(InputEvent::Key(Key::Tab)), PreferencesAction::None);
    let focus = dialog.focus.expect("focus");
    assert_eq!(dialog.rows()[focus].key, PrefKey::MusicLevel);
    assert_eq!(
        dialog.input(InputEvent::Key(Key::Char(' '))),
        PreferencesAction::None,
            "Space на слайдере ничего не делает"
    );

    assert_eq!(dialog.input(InputEvent::Key(Key::Enter)), PreferencesAction::Done);
    assert_eq!(dialog.input(InputEvent::Key(Key::Escape)), PreferencesAction::Cancel);
}

/// A click on a slider sets the value by the `UpdateSlider` formula.
#[test]
fn slider_click_sets_the_value_by_track_position() {
    let mut dialog = dialog(PreferencesVariant::InGame);
    let x = dialog
        .slider_screen_x(PrefKey::QuickScroll, 36)
        .expect("slider");
    let expected = dialog.slider_value_at(PrefKey::QuickScroll, x).unwrap();
    let rect = dialog.screen_rect(PrefKey::QuickScroll).expect("row");
    let y = rect.uly + rect.height / 2;

    assert_eq!(
        dialog.input(InputEvent::MouseClick(x, y)),
        PreferencesAction::Changed
    );
    assert_eq!(dialog.draft.quick_scroll, expected);
    assert!((expected - 36).abs() <= 1, "квантование жёлоба: {expected}");

    // Keyboard: Right/Left move the focused slider.
    dialog.focus = dialog
        .rows()
        .iter()
        .position(|row| row.key == PrefKey::QuickScroll);
    dialog.input(InputEvent::Key(Key::Right));
    assert_eq!(dialog.draft.quick_scroll, expected + 4);
}

/// Name field: click, input, `Enter` — the value into the draft (C++ `TextEdit`).
#[test]
fn player_name_edit_accepts_typing_and_enter() {
    let mut dialog = dialog(PreferencesVariant::Setup);
    let rect = dialog.screen_rect(PrefKey::PlayerName).expect("name");
    let (x, y) = (rect.ulx + 4, rect.uly + rect.height / 2);

    dialog.input(InputEvent::MouseClick(x, y));
    assert!(dialog.editing().is_some());

    for _ in 0..16 {
        dialog.input(InputEvent::Key(Key::Backspace));
    }
    for character in "Commander".chars() {
        dialog.input(InputEvent::Key(Key::Char(character)));
    }
    dialog.input(InputEvent::Key(Key::Enter));

    assert_eq!(dialog.draft.player_name, "Commander");
    assert!(dialog.editing().is_none());
}

/// `IniPreferences` — exactly 7 V70 fields.
#[test]
fn ini_preferences_roundtrip() {
    let mut prefs = Preferences::default();
    prefs.effects = false;
    prefs.click_scroll = false;
    prefs.quick_scroll = 40;
    prefs.fast_movement = false;
    prefs.follow_unit = true;
    prefs.auto_select = true;
    prefs.enemy_halt = false;

    let ini = prefs.to_ini();
    assert_eq!(
        ini,
        IniPreferences {
            effects: 0,
            click_scroll: 0,
            quick_scroll: 40,
            fast_movement: 0,
            follow_unit: 1,
            auto_select: 1,
            enemy_halt: 0,
        }
    );

    let mut restored = Preferences::default();
    restored.apply_ini(ini);
    assert_eq!(
        (
            restored.effects,
            restored.click_scroll,
            restored.quick_scroll,
            restored.fast_movement,
            restored.follow_unit,
            restored.auto_select,
            restored.enemy_halt,
        ),
        (false, false, 40, false, true, true, false)
    );
}

/// The settings file survives write/read (C++ `settings.json`).
#[test]
fn preferences_file_roundtrip() {
    let dir = std::env::temp_dir().join(format!(
            "max-brh-prefs-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or(0)
    ));
    let path = preferences_path_for(&dir);
    let mut prefs = Preferences::default();
    prefs.effects = false;
    prefs.quick_scroll = 40;
    prefs.disable_fx = true;
    prefs.player_name = "Cmdr".to_string();
    // Debug observation mode: the value survives the file round trip.
    prefs.no_fow = true;

    prefs.save_file(&path).expect("save");
    let loaded = Preferences::load_file(&path).expect("load");

    assert_eq!(loaded, prefs);
    assert!(loaded.no_fow, "no_fow must be read back from preferences.ini");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Campaign progression is persistent user state (`settings.cpp:102/105`,
/// plan M5): `last_campaign`/`game_file_number` are written to and read from
/// `preferences.ini` **byte for byte** — no rounding, clamping or defaults on
/// the way through the file.
#[test]
fn campaign_progress_roundtrips_the_settings_file_byte_for_byte() {
    let dir = std::env::temp_dir().join(format!(
            "max-brh-prefs-campaign-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or(0)
    ));
    let path = preferences_path_for(&dir);
    let second = dir.join("preferences-copy.ini");
    let mut prefs = Preferences::default();
    prefs.last_campaign = 4;
    prefs.game_file_number = 3;

    prefs.save_file(&path).expect("save");
    let bytes = std::fs::read(&path).expect("the settings file exists");

    let loaded = Preferences::load_file(&path).expect("load");
    assert_eq!(loaded, prefs, "the values survive write → read");
    assert_eq!(loaded.last_campaign, 4);
    assert_eq!(loaded.game_file_number, 3);

    // A second write of the loaded values produces the same bytes.
    loaded.save_file(&second).expect("second save");
    assert_eq!(
        std::fs::read(&second).expect("the copy exists"),
        bytes,
            "preferences.ini must round-trip byte for byte"
    );

    // `settings.cpp` defaults: 1/1 for a file without the keys (an old
    // `preferences.ini` of a client without the campaign stage).
    std::fs::write(&path, "player_name=Cmdr\n").expect("legacy file");
    let legacy = Preferences::load_file(&path).expect("legacy load");
    assert_eq!(legacy.last_campaign, 1);
    assert_eq!(legacy.game_file_number, 1);
    assert_eq!(legacy.player_name, "Cmdr");

    let _ = std::fs::remove_dir_all(&dir);
}

/// By default effects and sound are on (`settings.cpp`: `effects = 1`,
/// `disable_fx = 0`).
#[test]
fn defaults_match_settings_cpp() {
    let prefs = Preferences::default();
    assert!(prefs.effects);
    assert!(prefs.sound_enabled());
    assert!((prefs.sound_volume() - 1.0).abs() < f32::EPSILON);
    assert_eq!(prefs.quick_scroll, 16);
    assert!(prefs.auto_save);
    assert_eq!(prefs.turn_timer, 180);
    assert_eq!(prefs.endturn, 45);
}

/// `PrefKey::parse` understands the QA command names.
#[test]
fn prefs_key_names_roundtrip() {
    for key in [
        PrefKey::Effects,
        PrefKey::DisableFx,
        PrefKey::ClickScroll,
        PrefKey::QuickScroll,
        PrefKey::PlayerName,
        PrefKey::TurnTimer,
        PrefKey::EndTurn,
    ] {
        assert_eq!(PrefKey::parse(key.name()), Some(key), "{}", key.name());
    }
    assert_eq!(PrefKey::parse("sound"), Some(PrefKey::DisableFx));
    assert_eq!(PrefKey::parse("track_unit"), Some(PrefKey::FollowUnit));
    assert_eq!(PrefKey::parse("bogus"), None);
}
