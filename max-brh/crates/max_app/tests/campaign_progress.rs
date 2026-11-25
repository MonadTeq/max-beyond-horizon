//! Campaign stage 3a: persistent campaign progression (`last_campaign` /
//! `game_file_number`, plan M5).
//!
//! The progression is **user state**, not a UI flag: it lives in `Preferences`
//! (`preferences.ini`, the port of the C++ settings file, `settings.cpp:101-105`),
//! is advanced by the victory wrap-up (`menu.cpp:518-545`) and only read by the
//! `GameSetupMenu` campaign list (`gamesetupmenu.cpp:257-298`).
//!
//! Gates:
//! 1. winning Campaign 1 raises `last_campaign` to 2 and stores it on disk; a fresh
//!    application reads it back (the restart case);
//! 2. `preferences.ini` survives write → read → write byte for byte with the
//!    progression values (also covered by the unit test in `preferences.rs`);
//! 3. the campaign list unlocks exactly the missions below `last_campaign`: locked
//!    rows stay in the list, cannot be selected and never start;
//! 4. end-to-end Campaign 1 → 2 through the menu: win Campaign 1, return to the main
//!    menu, open the campaign list, start mission 2 — `snapshot_bytes` equals a
//!    direct `start_local_game_from_save` of `SAVE2.CAM`;
//! 5. a `last_campaign` above the list is clamped when the list opens and written
//!    back (`menu.cpp:1970-1982`), and a defeat never advances it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use max_app::campaign::CampaignMenuOpen;
use max_app::gamesetupmenu::MissionSelectForm;
use max_app::local::{LocalPhase, LocalSetup};
use max_app::menu::{self, MainMenuAction};
use max_app::newgame::{self, NewGameAction};
use max_app::preferences::Preferences;
use max_app::qa::QaMenuClient;
use max_app::ui::{InputEvent, Key, Screen, Transition, UiContext};
use max_app::{App, AppConfig};
use max_assets::MissionRegistry;
use max_core::enums::{FileType, UnitType};
use max_core::team::TeamId;
use max_core::victory::MissionCategory;
use max_render::buffer::PixelRect;

/// `max-original/SAVE1.CAM` — campaign 1 (`SC_C0001`).
const SAVE1_CAMPAIGN: &[u8] = include_bytes!("data/save1_v70.cam");

const OFF_FILE_TYPE: usize = 2;
const OFF_MISSION_INDEX: usize = 34;
/// The registry's campaign descriptors (`SC_C0001`…`SC_C0009`).
const CAMPAIGN_MISSIONS: usize = 9;

fn patches_res() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../max_assets/tests/data/PATCHES.RES")
}

fn reheader(source: &[u8], file_type: FileType, mission_index: u16) -> Vec<u8> {
    let mut bytes = source.to_vec();
    bytes[OFF_FILE_TYPE] = file_type as u8;
    bytes[OFF_MISSION_INDEX..OFF_MISSION_INDEX + 2].copy_from_slice(&mission_index.to_le_bytes());
    bytes
}

/// Temporary data directory: campaign 1 and a synthetic campaign 2.
///
/// Only `SAVE1.CAM` is committed; the V70 header is what identifies the mission
/// (`saveload.cpp:772`), so campaign 2 is the same world with `mission_index = 2`.
/// The registry still holds all nine descriptors (`SC_C0001`…`SC_C0009`), so the
/// list bound (`last_campaign` clamp) is the real one.
fn campaign_data_dir(name: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);

    let dir = std::env::temp_dir().join(format!(
        "max_app_campaign_{name}_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    std::fs::create_dir_all(&dir).expect("create temp data dir");
    std::fs::write(dir.join("SAVE1.CAM"), SAVE1_CAMPAIGN).expect("SAVE1.CAM");
    std::fs::write(
        dir.join("SAVE2.CAM"),
        reheader(SAVE1_CAMPAIGN, FileType::Campaign, 2),
    )
    .expect("SAVE2.CAM");

    dir
}

fn load_registry(dir: &Path) -> MissionRegistry {
    MissionRegistry::open(patches_res(), dir).expect("PATCHES.RES parses")
}

/// Application whose persistent settings and saves live in `dir`.
fn app_in(dir: &Path) -> App {
    App::new(
        AppConfig::default()
            .with_save_dir(dir)
            .with_save_data_dir(dir),
    )
}

fn mission_setup() -> LocalSetup {
    LocalSetup {
        player_name: "Commander".to_string(),
        ..LocalSetup::default()
    }
}

fn preferences_in(dir: &Path) -> PathBuf {
    max_app::preferences::preferences_path_for(dir)
}

// ---------------------------------------------------------------------------
// The campaign match: drive it to the mission's own win/loss outcome
// ---------------------------------------------------------------------------

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

fn run_until_finished_or_player(app: &mut App, limit: usize) {
    for _ in 0..limit {
        if app.local().is_none_or(|local| local.finished()) {
            return;
        }
        if app.local().map(|local| local.phase) == Some(LocalPhase::PlayerTurn) {
            return;
        }
        app.local_update(0.0);
    }
}

fn green_units(app: &App, unit_type: UnitType) -> Vec<max_core::unit::UnitId> {
    app.state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter(|id| {
            app.state_ref()
                .unit(*id)
                .is_some_and(|unit| unit.team == TeamId::GREEN && unit.unit_type == unit_type)
        })
        .collect()
}

fn destroy_green(app: &mut App, unit_type: UnitType) {
    for id in green_units(app, unit_type) {
        app.universe_mut().destroy_unit(id);
    }
}

/// `SC_C0001` victory (0 green houses at GREEN, a live research centre): destroy
/// the green houses and close the round. The AI may rebuild one, so the round loop
/// repeats until the mission reports the end of the match.
fn win_campaign_one(app: &mut App) {
    assert!(
        !green_units(app, UnitType::ResearchCentre).is_empty(),
        "SC_C0001: the defeat condition needs a green research centre"
    );
    assert!(
        !green_units(app, UnitType::EcoSphere).is_empty(),
        "SC_C0001: the victory condition needs a green house"
    );

    for _ in 0..8 {
        assert!(
            !app.local().is_some_and(|local| local.finished()),
            "the match ended before the round was closed"
        );
        assert!(
            run_until_player_turn(app, 40_000),
            "the local loop must reach the human turn"
        );

        destroy_green(app, UnitType::EcoSphere);
        assert!(
            !green_units(app, UnitType::ResearchCentre).is_empty(),
            "the green research centre must survive, otherwise SC_C0001 is a loss"
        );
        app.local_end_turn();
        run_until_finished_or_player(app, 40_000);

        if app.local().is_some_and(|local| local.finished()) {
            return;
        }
    }

    panic!("campaign 1 must end once the green houses are gone");
}

/// `SC_C0001` defeat (0 research centres at GREEN → RED loses).
fn lose_campaign_one(app: &mut App) {
    for _ in 0..8 {
        assert!(
            !app.local().is_some_and(|local| local.finished()),
            "the match ended before the round was closed"
        );
        assert!(run_until_player_turn(app, 40_000));

        destroy_green(app, UnitType::ResearchCentre);
        app.local_end_turn();
        run_until_finished_or_player(app, 40_000);

        if app.local().is_some_and(|local| local.finished()) {
            return;
        }
    }

    panic!("campaign 1 must end once the green research centres are gone");
}

fn outcome_is_victory(app: &App) -> bool {
    app.local()
        .and_then(|local| local.outcome.as_ref())
        .is_some_and(|outcome| outcome.victory)
}

/// The exact driver step of `main.rs`/`QaRunner` after a match: the victory screen
/// (`Screen::GameOver`) accepts `Enter` → [`Transition::LeaveSession`], the driver
/// calls `App::leave_local_game` (`menu.cpp:518-545` runs the campaign wrap-up there)
/// and syncs the menu mirror of the settings.
fn leave_through_game_over(app: &mut App) {
    let mut ctx = UiContext::new(Box::new(QaMenuClient), 640, 480);
    ctx.preferences = app.preferences().clone();
    let mut screen = Screen::GameOver;

    assert_eq!(
        screen.handle_input(&mut ctx, InputEvent::Key(Key::Enter)),
        Transition::LeaveSession,
        "the victory screen returns to the main menu"
    );

    app.leave_local_game();
    ctx.preferences = app.preferences().clone();

    assert_eq!(
        ctx.preferences.last_campaign,
        app.preferences().last_campaign,
        "the menu mirror follows the app"
    );
    assert_eq!(
        ctx.preferences.game_file_number,
        app.preferences().game_file_number
    );
}

// ---------------------------------------------------------------------------
// The menu layer, driven like `main.rs`/`QaRunner`
// ---------------------------------------------------------------------------

/// Main menu → `New Game` submenu → `Campaign Game` → GameSetupMenu.
struct Menu {
    ctx: UiContext,
    screen: Screen,
}

impl Menu {
    fn new(registry: Arc<MissionRegistry>, preferences: &Preferences) -> Self {
        let mut ctx = UiContext::new(Box::new(QaMenuClient), 640, 480);
        ctx.player_name = preferences.player_name.clone();
        ctx.preferences = preferences.clone();
        ctx.missions = Some(registry);

        Self {
            ctx,
            screen: Screen::MainMenu,
        }
    }

    /// One input event, plus the driver step of `main.rs`: the campaign list reports
    /// the `GameSetupMenu_Menu` clamp, the driver applies it.
    fn send(&mut self, app: &mut App, event: InputEvent) -> Transition {
        let transition = self.screen.handle_input(&mut self.ctx, event);

        if let Some(last_campaign) = self.ctx.campaign_clamp.take() {
            app.apply_campaign_clamp(last_campaign);
        }

        transition
    }

    fn click(&mut self, app: &mut App, rect: PixelRect) -> Transition {
        self.send(
            app,
            InputEvent::MouseClick(rect.ulx + rect.width / 2, rect.uly + rect.height / 2),
        )
    }

    fn key(&mut self, app: &mut App, key: Key) -> Transition {
        self.send(app, InputEvent::Key(key))
    }

    /// MainMenu → `New Game` → `Campaign Game`; returns the campaign form.
    fn open_campaign(&mut self, app: &mut App) -> &MissionSelectForm {
        let new_game = menu::main_menu_rect(MainMenuAction::Create).expect("New Game");
        self.click(app, new_game);
        assert!(
            matches!(self.screen, Screen::NewGameMenu { .. }),
            "New Game must open the submenu"
        );

        let campaign = newgame::new_game_rect(NewGameAction::Campaign).expect("Campaign Game");
        self.click(app, campaign);

        match &self.screen {
            Screen::MissionSelect(form) => form,
            other => panic!("expected MissionSelect, got {other:?}"),
        }
    }

    fn form(&self) -> &MissionSelectForm {
        match &self.screen {
            Screen::MissionSelect(form) => form,
            other => panic!("expected MissionSelect, got {other:?}"),
        }
    }
}

/// Executes `Transition::StartMission` like `main.rs`/`QaRunner`.
fn execute_start(
    menu: &mut Menu,
    app: &mut App,
    transition: Transition,
) -> max_app::mission::MissionStart {
    let Transition::StartMission(request) = transition else {
        panic!("the menu must emit StartMission, got {transition:?}");
    };
    let registry = menu.ctx.missions.as_deref().expect("registry");

    app.start_mission_from_registry(registry, request.category, request.index, &mission_setup())
        .expect("mission starts")
}

// ---------------------------------------------------------------------------
// Gate 1–2: victory → persistent last_campaign, restart, byte round-trip
// ---------------------------------------------------------------------------

#[test]
fn campaign_one_victory_advances_the_persistent_progression() {
    let dir = campaign_data_dir("victory");
    let registry = load_registry(&dir);
    let mut app = app_in(&dir);

    assert_eq!(app.preferences().last_campaign, 1);
    assert_eq!(app.preferences().game_file_number, 1);

    app.start_mission_from_registry(&registry, MissionCategory::Campaign, 0, &mission_setup())
        .expect("campaign 1 starts");
    assert_eq!(app.state_ref().mission_index(), 1);
    assert_eq!(app.campaign_mission_count(), Some(CAMPAIGN_MISSIONS));

    win_campaign_one(&mut app);
    assert!(outcome_is_victory(&app), "campaign 1 must be won");

    // `menu.cpp:518-545`: the wrap-up (here the return to the main menu) advances
    // `game_file_number` to the next mission and raises `last_campaign`.
    leave_through_game_over(&mut app);

    assert_eq!(
        app.preferences().last_campaign,
        2,
        "a won campaign mission unlocks the next one"
    );
    assert_eq!(app.preferences().game_file_number, 2);

    // The values are on disk and a fresh application (a restart) reads them back.
    let path = preferences_in(&dir);
    let stored = Preferences::load_file(&path).expect("preferences.ini was written");
    assert_eq!(stored.last_campaign, 2);
    assert_eq!(stored.game_file_number, 2);
    assert_eq!(stored, *app.preferences());

    let mut restarted = app_in(&dir);
    assert!(restarted.load_preferences_from_disk());
    assert_eq!(
        restarted.preferences().last_campaign,
        2,
        "the progression survives a restart"
    );
    assert_eq!(restarted.preferences().game_file_number, 2);

    // Byte-for-byte round trip: write the loaded settings again — the bytes match.
    let first = std::fs::read(&path).expect("the settings file exists");
    let copy = dir.join("preferences-copy.ini");
    stored.save_file(&copy).expect("second write");
    assert_eq!(
        std::fs::read(&copy).expect("the copy exists"),
        first,
        "preferences.ini must round-trip byte for byte"
    );

    // A replay of mission 1 must not lower the high-water mark
    // (`menu.cpp:534-536`: only `last_campaign < mission_index + 1` raises it).
    restarted
        .start_mission_from_registry(&registry, MissionCategory::Campaign, 0, &mission_setup())
        .expect("campaign 1 restarts");
    win_campaign_one(&mut restarted);
    assert!(outcome_is_victory(&restarted));
    leave_through_game_over(&mut restarted);
    assert_eq!(restarted.preferences().last_campaign, 2);
    assert_eq!(restarted.preferences().game_file_number, 2);

    std::fs::remove_dir_all(&dir).ok();
}

/// A defeat never advances the progression (`menu.cpp:518`: `if (is_winner)`).
#[test]
fn campaign_defeat_keeps_the_progression() {
    let dir = campaign_data_dir("defeat");
    let registry = load_registry(&dir);
    let mut app = app_in(&dir);

    app.start_mission_from_registry(&registry, MissionCategory::Campaign, 0, &mission_setup())
        .expect("campaign 1 starts");
    lose_campaign_one(&mut app);
    assert!(!outcome_is_victory(&app), "campaign 1 must be lost");

    leave_through_game_over(&mut app);

    assert_eq!(app.preferences().last_campaign, 1);
    assert_eq!(app.preferences().game_file_number, 1);
    assert!(
        !preferences_in(&dir).exists(),
        "a defeat must not rewrite the settings"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Gate 3–4: the campaign list unlocks exactly `last_campaign`; Campaign 1 → 2
// ---------------------------------------------------------------------------

#[test]
fn campaign_menu_unlocks_only_the_missions_below_last_campaign() {
    let dir = campaign_data_dir("menu");
    let registry = Arc::new(load_registry(&dir));

    // State of the settings after winning campaign 1 (the previous test covers how
    // they got there).
    let mut app = app_in(&dir);
    let mut prefs = Preferences::default();
    prefs.last_campaign = 2;
    prefs.game_file_number = 2;
    app.set_preferences(prefs.clone());
    app.persist_preferences();

    let mut menu = Menu::new(Arc::clone(&registry), &prefs);
    let form = menu.open_campaign(&mut app).clone();

    assert_eq!(form.category, MissionCategory::Campaign);
    assert_eq!(form.unlocked_count(Some(&registry)), 2);
    assert_eq!(
        form.selected, 1,
        "opens on `game_file_number - 1` = mission 2"
    );
    assert!(!form.is_locked(Some(&registry), 0));
    assert!(!form.is_locked(Some(&registry), 1));
    assert!(form.is_locked(Some(&registry), 2));

    // Locked rows stay in the list (the descriptor is there, the oracle only leaves
    // the title empty and disables the button): mission 3 is visible as a row.
    assert!(
        form.row_descriptor(Some(&registry), 2).is_some(),
        "a locked campaign row is still part of the list"
    );

    // A click on the locked row is ignored (`DrawMissionList` disables the button).
    assert_eq!(
        menu.click(&mut app, max_app::gamesetupmenu::row_rect(2)),
        Transition::None
    );
    assert_eq!(
        menu.form().selected,
        1,
        "a locked row must not change the selection"
    );

    // ... and a locked mission can never start, even if the selection is forced.
    let mut forced = menu.form().clone();
    forced.selected = 2;
    assert!(!forced.can_start(Some(&registry)));
    assert!(forced.start_error(Some(&registry)).contains("locked"));

    // `Start` on the selected, unlocked mission 2 (`game_file_number = 2`).
    let transition = menu.key(&mut app, Key::Enter);
    let start = execute_start(&mut menu, &mut app, transition);
    assert_eq!(start.category, MissionCategory::Campaign);
    assert_eq!(start.mission_index, 2);
    assert!(
        start
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("SAVE2.CAM")),
        "started {:?}, expected SAVE2.CAM",
        start.path
    );
    assert_eq!(app.preferences().game_file_number, 2);

    // End-to-end gate: the menu start of campaign 2 is byte-identical to a direct
    // V70 load of `SAVE2.CAM` (at turn zero and after five ticks).
    let bytes = std::fs::read(dir.join("SAVE2.CAM")).expect("SAVE2.CAM");
    let mut direct = app_in(&dir);
    direct
        .start_local_game_from_save(&mission_setup(), &bytes)
        .expect("direct load of campaign 2");

    assert_eq!(
        app.snapshot(),
        direct.snapshot(),
        "the campaign menu start must equal a direct load byte for byte"
    );
    assert_eq!(app.digest(), direct.digest());

    for _ in 0..5 {
        app.tick();
        direct.tick();
    }

    assert_eq!(
        app.snapshot(),
        direct.snapshot(),
        "5 ticks of campaign 2 diverge from the direct load"
    );

    // Opening the list and starting the mission does not move the stored progression.
    assert_eq!(app.preferences().last_campaign, 2);

    std::fs::remove_dir_all(&dir).ok();
}

/// `last_campaign` above the mission list: the list clamps it, opens on the last
/// mission and reports the clamp the driver writes back (`menu.cpp:1970-1982`).
#[test]
fn campaign_menu_clamps_last_campaign_and_the_driver_writes_it_back() {
    let dir = campaign_data_dir("clamp");
    let registry = Arc::new(load_registry(&dir));

    let mut app = app_in(&dir);
    let mut prefs = Preferences::default();
    prefs.last_campaign = 99;
    prefs.game_file_number = 4;
    app.set_preferences(prefs.clone());
    app.persist_preferences();

    let mut menu = Menu::new(Arc::clone(&registry), &prefs);
    let form = menu.open_campaign(&mut app).clone();

    assert_eq!(
        form.unlocked_count(Some(&registry)),
        CAMPAIGN_MISSIONS,
        "all nine missions are unlocked after the clamp"
    );
    assert_eq!(
        form.selected,
        CAMPAIGN_MISSIONS - 1,
        "the list opens on the clamped `last_campaign - 1`"
    );
    assert_eq!(menu.ctx.preferences.last_campaign, CAMPAIGN_MISSIONS as i32);
    assert_eq!(
        menu.ctx.preferences.game_file_number,
        CAMPAIGN_MISSIONS as i32
    );
    assert_eq!(
        menu.ctx.campaign_clamp, None,
        "the driver consumed the reported clamp"
    );

    // `Menu::send` applied it like `main.rs`: both settings and the file follow.
    let stored = Preferences::load_file(&preferences_in(&dir)).expect("settings file");
    assert_eq!(stored.last_campaign, CAMPAIGN_MISSIONS as i32);
    assert_eq!(stored.game_file_number, CAMPAIGN_MISSIONS as i32);
    assert_eq!(app.preferences().last_campaign, CAMPAIGN_MISSIONS as i32);
    assert_eq!(app.preferences().game_file_number, CAMPAIGN_MISSIONS as i32);

    std::fs::remove_dir_all(&dir).ok();
}

/// A campaign list without the registry (owner condition 5 fallback) must not
/// destroy the stored progression (the oracle would clamp against an unknown zero).
#[test]
fn campaign_menu_without_registry_keeps_the_progression() {
    let mut form = MissionSelectForm::new(MissionCategory::Campaign);
    let mut prefs = Preferences::default();
    prefs.last_campaign = 3;

    assert!(form.open_campaign(prefs.last_campaign, None).is_none());
    assert_eq!(form.unlocked_count(None), 0);
    assert_eq!(form.campaign_unlocked, 0);
    assert_eq!(prefs.last_campaign, 3, "the stored value is untouched");
}

/// The progression does not reach the simulation: `last_campaign`/`game_file_number`
/// are not part of `snapshot_bytes`/CRC (they are user settings).
#[test]
fn campaign_settings_do_not_change_the_snapshot() {
    let dir = campaign_data_dir("snapshot");
    let bytes = std::fs::read(dir.join("SAVE1.CAM")).expect("SAVE1.CAM");

    let mut low = app_in(&dir);
    low.start_local_game_from_save(&mission_setup(), &bytes)
        .expect("direct load");

    let mut high = app_in(&dir);
    let mut prefs = Preferences::default();
    prefs.last_campaign = 7;
    prefs.game_file_number = 5;
    high.set_preferences(prefs);
    high.start_local_game_from_save(&mission_setup(), &bytes)
        .expect("direct load");

    assert_eq!(
        low.snapshot(),
        high.snapshot(),
        "campaign progression is user state, not simulation state"
    );
    assert_eq!(low.digest(), high.digest());

    std::fs::remove_dir_all(&dir).ok();
}

/// The campaign open result of the oracle clamp is also exposed as a DTO for the
/// menu-side checks.
#[test]
fn campaign_menu_open_dto_is_zero_based_and_clamped() {
    let open = max_app::campaign::menu_open(2, CAMPAIGN_MISSIONS);
    assert_eq!(
        open,
        CampaignMenuOpen {
            last_campaign: 2,
            selected: 1,
            clamped: false,
        }
    );
}
