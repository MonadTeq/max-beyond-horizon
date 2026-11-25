//! Campaign stage 2: the `New Game` submenu (`menu_new_game_menu_loop`) and the
//! `GameSetupMenu` mission list (`gamesetupmenu.cpp`).
//!
//! The screen machine is driven with the same `InputEvent`s as the mouse/keyboard;
//! the only thing the menu is allowed to do on start is emit
//! [`Transition::StartMission`]. The driver here executes that transition exactly like
//! `main.rs`/`QaRunner`: [`App::start_mission_from_registry`] (stage 1).
//!
//! Owner gates:
//! 1. the new UI code never mutates the application (source audit + the start flows
//!    through the install API only);
//! 2. `GameSetupMenu` produces a start DTO and mutates nothing;
//! 3. every mission-category button starts its category, and the result is
//!    byte-identical to a direct `start_local_game_from_save` of the mission file;
//! 4. navigating/opening/closing the menus N times does not change `snapshot_bytes`;
//! 5. a missing mission file is visible (`[missing]`, description, status line) and
//!    never starts an empty world.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use max_app::gamesetupmenu::{self, MissionSelectForm};
use max_app::local::LocalSetup;
use max_app::menu::{self, MainMenuAction};
use max_app::newgame::{self, NewGameAction, TipsWindow};
use max_app::qa::QaMenuClient;
use max_app::ui::{InputEvent, Key, Screen, Transition, UiContext};
use max_app::{App, AppConfig};
use max_assets::MissionRegistry;
use max_core::enums::FileType;
use max_core::victory::MissionCategory;
use max_render::buffer::PixelRect;

/// `max-original/SAVE1.CAM` — campaign 1 (`SC_C0001`).
const SAVE1_CAMPAIGN: &[u8] = include_bytes!("data/save1_v70.cam");
/// `max-original/SAVE18.SCE` — scenario 18 (`SC_S0018`).
const SAVE18_SCENARIO: &[u8] = include_bytes!("../../max_core/tests/data/save18_v70.sce");

const OFF_FILE_TYPE: usize = 2;
const OFF_MISSION_INDEX: usize = 34;

fn patches_res() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../max_assets/tests/data/PATCHES.RES")
}

fn reheader(source: &[u8], file_type: FileType, mission_index: u16) -> Vec<u8> {
    let mut bytes = source.to_vec();
    bytes[OFF_FILE_TYPE] = file_type as u8;
    bytes[OFF_MISSION_INDEX..OFF_MISSION_INDEX + 2].copy_from_slice(&mission_index.to_le_bytes());
    bytes
}

/// Temporary mission data directory with the four mission files.
fn mission_data_dir(name: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);

    let dir = std::env::temp_dir().join(format!(
        "max_app_game_setup_{name}_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    std::fs::create_dir_all(&dir).expect("create temp data dir");
    std::fs::write(dir.join("SAVE1.CAM"), SAVE1_CAMPAIGN).expect("SAVE1.CAM");
    std::fs::write(dir.join("SAVE18.SCE"), SAVE18_SCENARIO).expect("SAVE18.SCE");
    std::fs::write(
        dir.join("SAVE1.TRA"),
        reheader(SAVE1_CAMPAIGN, FileType::Tutorial, 1),
    )
    .expect("SAVE1.TRA");
    std::fs::write(
        dir.join("SAVE1.MPS"),
        reheader(SAVE1_CAMPAIGN, FileType::MultiScenario, 1),
    )
    .expect("SAVE1.MPS");

    dir
}

fn load_registry(dir: &Path) -> MissionRegistry {
    MissionRegistry::open(patches_res(), dir).expect("PATCHES.RES parses")
}

/// Menu screen machine driven exactly like `main.rs`: `Screen::handle_input`.
struct MenuUi {
    ctx: UiContext,
    screen: Screen,
}

impl MenuUi {
    fn new(registry: Arc<MissionRegistry>) -> Self {
        let mut ctx = UiContext::new(Box::new(QaMenuClient), 640, 480);
        ctx.player_name = "Commander".to_string();
        ctx.preferences.player_name = "Commander".to_string();
        ctx.missions = Some(registry);

        Self {
            ctx,
            screen: Screen::MainMenu,
        }
    }

    fn send(&mut self, event: InputEvent) -> Transition {
        self.screen.handle_input(&mut self.ctx, event)
    }

    fn click(&mut self, rect: PixelRect) -> Transition {
        self.send(InputEvent::MouseClick(
            rect.ulx + rect.width / 2,
            rect.uly + rect.height / 2,
        ))
    }

    fn key(&mut self, key: Key) -> Transition {
        self.send(InputEvent::Key(key))
    }

    fn open_new_game(&mut self) -> Transition {
        let rect = menu::main_menu_rect(MainMenuAction::Create).expect("New Game");
        self.click(rect)
    }

    /// `MainMenu` → `New Game` submenu → category button.
    fn open_category(&mut self, action: NewGameAction) {
        self.open_new_game();
        assert!(
            matches!(self.screen, Screen::NewGameMenu { .. }),
            "New Game must open the submenu"
        );

        let rect = newgame::new_game_rect(action).expect("category button");
        self.click(rect);
        assert!(
            matches!(self.screen, Screen::MissionSelect(_)),
            "{action:?} must open GameSetupMenu"
        );
    }

    fn form(&self) -> &MissionSelectForm {
        match &self.screen {
            Screen::MissionSelect(form) => form,
            other => panic!("expected MissionSelect, got {other:?}"),
        }
    }

    fn registry(&self) -> &MissionRegistry {
        self.ctx.missions.as_deref().expect("registry")
    }
}

/// Executes `Transition::StartMission` like `main.rs`/`QaRunner`.
fn execute_start(
    ui: &mut MenuUi,
    app: &mut App,
    transition: Transition,
) -> max_app::mission::MissionStart {
    let Transition::StartMission(request) = transition else {
        panic!("the menu must emit StartMission, got {transition:?}");
    };
    let setup = LocalSetup {
        player_name: ui.ctx.player_name.clone(),
        ..LocalSetup::default()
    };

    app.start_mission_from_registry(ui.registry(), request.category, request.index, &setup)
        .expect("mission starts")
}

// ---------------------------------------------------------------------------
// Gate 3: the seven buttons; every mission category starts like a direct load
// ---------------------------------------------------------------------------

/// `new_game_menu_buttons` (`menu.cpp:2181-2189`): six big 200×29 buttons at
/// x=385, y=175 step 35, and the small `Cancel` 100×30 at (435,420).
#[test]
fn new_game_menu_has_the_seven_oracle_buttons() {
    assert_eq!(newgame::NEW_GAME_BUTTONS.len(), 7);

    for (index, button) in newgame::NEW_GAME_BUTTONS.iter().take(6).enumerate() {
        assert_eq!(
            button.rect,
            PixelRect::new(385, 175 + index as i32 * 35, 200, 29),
            "{:?}",
            button.action
        );
        assert!(button.big);
    }

    let cancel = newgame::NEW_GAME_BUTTONS[6];
    assert_eq!(cancel.rect, PixelRect::new(435, 420, 100, 30));
    assert!(!cancel.big);
    assert_eq!(cancel.action, NewGameAction::Cancel);
    assert_eq!(cancel.label, "Cancel");

    assert_eq!(newgame::NEW_GAME_BUTTONS[0].label, "Training Missions");
    assert_eq!(newgame::NEW_GAME_BUTTONS[1].label, "Stand Alone Missions");
    assert_eq!(newgame::NEW_GAME_BUTTONS[2].label, "Custom Game");
    assert_eq!(newgame::NEW_GAME_BUTTONS[3].label, "Custom Scenarios");
    assert_eq!(newgame::NEW_GAME_BUTTONS[4].label, "Campaign Game");
    assert_eq!(newgame::NEW_GAME_BUTTONS[5].label, "Tips");

    // Hit test of the oracle rectangles.
    assert_eq!(
        newgame::new_game_button_at(385 + 100, 175 + 14),
        Some(NewGameAction::Training)
    );
    assert_eq!(
        newgame::new_game_button_at(435 + 50, 420 + 15),
        Some(NewGameAction::Cancel)
    );
    assert_eq!(newgame::new_game_button_at(100, 100), None);
}

/// Owner gate 3: each of the four mission-category buttons starts its category, and
/// the result is byte-identical to a direct `start_local_game_from_save`.
#[test]
fn every_category_button_starts_the_same_bytes_as_a_direct_load() {
    let dir = mission_data_dir("categories");
    let registry = Arc::new(load_registry(&dir));

    // `(button, registry index, mission file, page-downs needed)`. Scenario 18 is on
    // the second page of twelve rows — the list paging is exercised too.
    let cases = [
        (NewGameAction::Training, 0usize, "SAVE1.TRA", 0usize),
        (NewGameAction::Scenario, 17, "SAVE18.SCE", 1),
        (NewGameAction::MultiplayerScenario, 0, "SAVE1.MPS", 0),
        (NewGameAction::Campaign, 0, "SAVE1.CAM", 0),
    ];

    for (button, index, file, page_downs) in cases {
        let mut ui = MenuUi::new(Arc::clone(&registry));
        ui.open_category(button);

        for _ in 0..page_downs {
            ui.key(Key::PageDown);
        }

        // Select the mission through the list (the description follows the
        // selection, so `Start` starts exactly this mission).
        let row = index % gamesetupmenu::MISSIONS_PER_PAGE;
        let row_rect = gamesetupmenu::row_rect(row);
        ui.click(row_rect);

        assert_eq!(
            ui.form().selected,
            index,
            "{button:?}: click on row {row} must select mission {index}"
        );

        let mut app = App::new(AppConfig::default());
        let transition = ui.key(Key::Enter);
        let start = execute_start(&mut ui, &mut app, transition);

        assert_eq!(start.category, button_category(button), "{button:?}");
        assert!(
            start
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(file)),
            "{button:?}: started {:?}, expected {file}",
            start.path
        );
        assert!(app.local_active(), "{button:?}: no local match");

        // Direct load of the same file: the stage-1 equivalence gate.
        let bytes = std::fs::read(dir.join(file)).expect("mission file");
        let mut direct = App::new(AppConfig::default());
        direct
            .start_local_game_from_save(&LocalSetup::default(), &bytes)
            .unwrap_or_else(|error| panic!("{button:?}: direct load: {error}"));

        assert_eq!(
            app.snapshot(),
            direct.snapshot(),
            "{button:?}: the menu start must equal a direct V70 load byte for byte"
        );
        assert_eq!(app.digest(), direct.digest(), "{button:?}");

        // Five ticks stay identical too (the menu path does not perturb the match).
        for _ in 0..5 {
            app.tick();
            direct.tick();
        }

        assert_eq!(
            app.snapshot(),
            direct.snapshot(),
            "{button:?}: 5 ticks diverge from the direct load"
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}

fn button_category(action: NewGameAction) -> MissionCategory {
    match action {
        NewGameAction::Training => MissionCategory::Training,
        NewGameAction::Scenario => MissionCategory::Scenario,
        NewGameAction::MultiplayerScenario => MissionCategory::MultiPlayerScenario,
        NewGameAction::Campaign => MissionCategory::Campaign,
        other => panic!("{other:?} is not a mission category"),
    }
}

/// The `Custom` button keeps the old M-SP path (`menu_custom_game_menu`): the local
/// setup screens, not a mission list.
#[test]
fn custom_game_button_opens_the_planet_setup_and_cancels_back() {
    let dir = mission_data_dir("custom");
    let registry = Arc::new(load_registry(&dir));
    let mut ui = MenuUi::new(registry);

    ui.open_new_game();
    let custom = newgame::new_game_rect(NewGameAction::Custom).expect("Custom");
    ui.click(custom);

    assert!(matches!(ui.screen, Screen::LocalSetup(_)));

    // Cancel → `New Game` submenu (`menu_custom_game_menu` returns into the loop).
    assert_eq!(ui.key(Key::Escape), Transition::None);
    assert_eq!(ui.screen, Screen::NewGameMenu { tips: None });

    std::fs::remove_dir_all(&dir).ok();
}

/// `Cancel` returns to the main menu; the `Tips` button toggles the tips window.
#[test]
fn cancel_goes_back_and_tips_toggle_the_window() {
    let dir = mission_data_dir("tips");
    let registry = Arc::new(load_registry(&dir));
    let mut ui = MenuUi::new(registry);

    ui.open_new_game();

    // Tips window (`menu_draw_tips`): the oracle text, a page fits 23 rows.
    let tips = newgame::new_game_rect(NewGameAction::Tips).expect("Tips");
    ui.click(tips);
    let window = match &ui.screen {
        Screen::NewGameMenu { tips: Some(window) } => window.clone(),
        other => panic!("expected the tips window, got {other:?}"),
    };
    assert!(!window.rows().is_empty(), "the tips text must be loaded");
    assert_eq!(TipsWindow::rows_per_page(), 23);
    assert_eq!(window.row_index(), 0);
    assert!(window.can_scroll(true), "the tips must have a second page");

    // A click on the page arrow scrolls; the second Tips click closes the window.
    ui.click(gamesetupmenu::SCROLL_DOWN);
    let window = match &ui.screen {
        Screen::NewGameMenu { tips: Some(window) } => window.clone(),
        other => panic!("expected the tips window, got {other:?}"),
    };
    assert_eq!(window.row_index(), TipsWindow::rows_per_page());

    ui.click(tips);
    assert_eq!(ui.screen, Screen::NewGameMenu { tips: None });

    // Cancel (`Shift+C`/`Esc`).
    assert_eq!(ui.key(Key::Escape), Transition::None);
    assert_eq!(ui.screen, Screen::MainMenu);

    std::fs::remove_dir_all(&dir).ok();
}

/// `Shift+T/S/U/M/A/I/C` of the oracle (`menu.cpp:2213-2286`).
#[test]
fn new_game_hotkeys_match_the_oracle() {
    let dir = mission_data_dir("hotkeys");
    let registry = Arc::new(load_registry(&dir));

    for (key, category) in [
        ('t', MissionCategory::Training),
        ('s', MissionCategory::Scenario),
        ('m', MissionCategory::MultiPlayerScenario),
        ('a', MissionCategory::Campaign),
    ] {
        let mut ui = MenuUi::new(Arc::clone(&registry));
        ui.open_new_game();
        ui.key(Key::Char(key));
        assert_eq!(ui.form().category, category, "hotkey {key}");
    }

    // `U` — Custom, `I` — Tips, `C`/Esc — Cancel.
    let mut ui = MenuUi::new(Arc::clone(&registry));
    ui.open_new_game();
    ui.key(Key::Char('u'));
    assert!(matches!(ui.screen, Screen::LocalSetup(_)));

    let mut ui = MenuUi::new(Arc::clone(&registry));
    ui.open_new_game();
    ui.key(Key::Char('i'));
    assert!(matches!(ui.screen, Screen::NewGameMenu { tips: Some(_) }));

    ui.key(Key::Char('c'));
    assert_eq!(ui.screen, Screen::MainMenu);

    std::fs::remove_dir_all(&dir).ok();
}

/// The list is the C++ one: 12 rows of 294×19, page/briefing arrows and the bottom
/// buttons at the `gamesetupmenu.hpp` coordinates.
#[test]
fn mission_list_layout_matches_the_cpp_header() {
    assert_eq!(gamesetupmenu::row_rect(0), PixelRect::new(18, 184, 294, 19));
    assert_eq!(
        gamesetupmenu::row_rect(11),
        PixelRect::new(18, 404, 294, 19)
    );
    assert_eq!(gamesetupmenu::MISSIONS_PER_PAGE, 12);
    assert_eq!(gamesetupmenu::row_at(20, 190), Some(0));
    assert_eq!(gamesetupmenu::row_at(20, 410), Some(11));
    assert_eq!(gamesetupmenu::row_at(20, 430), None);

    assert_eq!(gamesetupmenu::SCROLL_UP, PixelRect::new(16, 438, 28, 29));
    assert_eq!(gamesetupmenu::SCROLL_DOWN, PixelRect::new(48, 438, 28, 29));
    assert_eq!(gamesetupmenu::BRIEF_UP, PixelRect::new(563, 438, 28, 29));
    assert_eq!(gamesetupmenu::BRIEF_DOWN, PixelRect::new(596, 438, 28, 29));
    assert_eq!(gamesetupmenu::CANCEL, PixelRect::new(200, 438, 109, 40));
    assert_eq!(gamesetupmenu::HELP, PixelRect::new(312, 438, 48, 40));
    assert_eq!(gamesetupmenu::START, PixelRect::new(361, 438, 109, 40));

    assert_eq!(
        gamesetupmenu::DESCRIPTION_TITLE,
        PixelRect::new(400, 174, 180, 21)
    );
    assert_eq!(
        gamesetupmenu::DESCRIPTION_TEXT,
        PixelRect::new(354, 215, 266, 201)
    );

    // The hit test maps the bottom row to the oracle events.
    use gamesetupmenu::MissionSelectAction;
    assert_eq!(
        gamesetupmenu::mission_select_at(30, 450),
        MissionSelectAction::PageUp
    );
    assert_eq!(
        gamesetupmenu::mission_select_at(60, 450),
        MissionSelectAction::PageDown
    );
    assert_eq!(
        gamesetupmenu::mission_select_at(570, 450),
        MissionSelectAction::BriefUp
    );
    assert_eq!(
        gamesetupmenu::mission_select_at(610, 450),
        MissionSelectAction::BriefDown
    );
    assert_eq!(
        gamesetupmenu::mission_select_at(250, 450),
        MissionSelectAction::Cancel
    );
    assert_eq!(
        gamesetupmenu::mission_select_at(330, 450),
        MissionSelectAction::Help
    );
    assert_eq!(
        gamesetupmenu::mission_select_at(400, 450),
        MissionSelectAction::Start
    );
}

/// The category titles and help sections of `GameSetupMenu::Init/EventHelp`.
#[test]
fn mission_category_titles_and_help_sections_match_the_oracle() {
    let campaign = MissionSelectForm::new(MissionCategory::Campaign);
    assert_eq!(campaign.title(), "Campaign Game");
    assert_eq!(
        campaign.help_section(),
        max_app::helpmenu::HelpSection::CampaignMenu
    );

    assert_eq!(
        MissionSelectForm::new(MissionCategory::Training).title(),
        "Training Missions"
    );
    assert_eq!(
        MissionSelectForm::new(MissionCategory::Training).help_section(),
        max_app::helpmenu::HelpSection::TrainingMenu
    );
    assert_eq!(
        MissionSelectForm::new(MissionCategory::Scenario).title(),
        "Stand Alone Missions"
    );
    assert_eq!(
        MissionSelectForm::new(MissionCategory::Scenario).help_section(),
        max_app::helpmenu::HelpSection::StandAloneMenu
    );
    assert_eq!(
        MissionSelectForm::new(MissionCategory::MultiPlayerScenario).title(),
        "Custom Scenarios"
    );
    assert_eq!(
        MissionSelectForm::new(MissionCategory::MultiPlayerScenario).help_section(),
        max_app::helpmenu::HelpSection::MultiScenarioMenu
    );
}

// ---------------------------------------------------------------------------
// Gate 4: opening/closing the menus does not change `snapshot_bytes`
// ---------------------------------------------------------------------------

/// Owner gate 4: N open/cancel cycles before the start leave `snapshot_bytes`
/// identical to a direct load (and to each other).
#[test]
fn menu_navigation_does_not_change_the_snapshot_bytes() {
    let dir = mission_data_dir("snapshot");
    let registry = Arc::new(load_registry(&dir));

    let bytes = std::fs::read(dir.join("SAVE1.CAM")).expect("SAVE1.CAM");
    let mut direct = App::new(AppConfig::default());
    direct
        .start_local_game_from_save(&LocalSetup::default(), &bytes)
        .expect("direct load");
    let baseline = direct.snapshot();

    for cycles in [0usize, 1, 3, 7] {
        let mut ui = MenuUi::new(Arc::clone(&registry));

        for _ in 0..cycles {
            ui.open_category(NewGameAction::Campaign);
            ui.key(Key::Escape);
            assert_eq!(ui.screen, Screen::NewGameMenu { tips: None });
            ui.key(Key::Escape);
            assert_eq!(ui.screen, Screen::MainMenu);
        }

        ui.open_category(NewGameAction::Campaign);
        let mut app = App::new(AppConfig::default());
        let transition = ui.key(Key::Enter);
        let start = execute_start(&mut ui, &mut app, transition);

        assert_eq!(start.category, MissionCategory::Campaign);
        assert_eq!(
            app.snapshot(),
            baseline,
            "{cycles} menu cycles changed snapshot_bytes"
        );
        assert_eq!(app.digest(), direct.digest(), "{cycles} menu cycles");
    }

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// Gate 5: a missing mission file is visible and never starts
// ---------------------------------------------------------------------------

/// Owner gate 5: with `SAVE1.TRA` absent the training list still renders the
/// descriptor, marks it `[missing]`, names the file in the description, and `Start`
/// refuses with a visible status — no empty world, no panic.
#[test]
fn missing_mission_file_is_visible_and_never_starts() {
    let dir = mission_data_dir("missing");
    std::fs::remove_file(dir.join("SAVE1.TRA")).expect("remove SAVE1.TRA");
    let registry = Arc::new(load_registry(&dir));
    let mut ui = MenuUi::new(registry);

    assert!(
        ui.ctx
            .missions
            .as_ref()
            .expect("registry")
            .warnings()
            .iter()
            .any(|warning| warning.contains("SAVE1.TRA")),
        "the registry must warn about the missing mission file"
    );

    ui.open_category(NewGameAction::Training);
    let form = ui.form().clone();
    assert_eq!(form.mission_count(Some(ui.registry())), 15);

    let descriptor = form
        .selected_descriptor(Some(ui.registry()))
        .expect("descriptors are kept despite the missing file");
    assert!(!descriptor.mission_found);
    assert_eq!(descriptor.mission_name.as_deref(), Some("SAVE1.TRA"));
    assert!(
        gamesetupmenu::row_title(0, descriptor).contains("[missing]"),
        "the row must carry the visible marker"
    );

    let lines = form.description_lines(Some(ui.registry()));
    assert!(
        lines.iter().any(|line| line.contains("SAVE1.TRA")
            && line.to_ascii_lowercase().contains("missing")),
        "the description must name the missing file: {lines:?}"
    );

    // `Start` stays on the screen with the reason; nothing is started.
    let app = App::new(AppConfig::default());
    assert_eq!(ui.key(Key::Enter), Transition::None);
    assert!(matches!(ui.screen, Screen::MissionSelect(_)));
    assert!(
        ui.ctx.status.contains("SAVE1.TRA"),
        "status: {}",
        ui.ctx.status
    );
    assert!(
        !app.local_active(),
        "a missing mission must not start a match"
    );

    // The list still shows the twelve rows of the page (14 of 15 files are missing
    // in this fixture): nothing panics, every row has a title.
    let page_rows = form.page_end(Some(ui.registry()));
    assert_eq!(page_rows, 12);
    for row in 0..page_rows {
        let descriptor = form
            .row_descriptor(Some(ui.registry()), row)
            .expect("row descriptor");
        assert!(!gamesetupmenu::row_title(row + form.page, descriptor).is_empty());
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// No registry at all (`PATCHES.RES` did not load): the screen shows the fallback and
/// `Start` refuses instead of panicking.
#[test]
fn absent_registry_is_a_visible_fallback() {
    let mut ctx = UiContext::new(Box::new(QaMenuClient), 640, 480);
    ctx.missions = None;
    let mut screen = Screen::MainMenu;

    let new_game = menu::main_menu_rect(MainMenuAction::Create).expect("New Game");
    screen.handle_input(
        &mut ctx,
        InputEvent::MouseClick(new_game.ulx + 4, new_game.uly + 4),
    );
    let campaign = newgame::new_game_rect(NewGameAction::Campaign).expect("Campaign");
    screen.handle_input(
        &mut ctx,
        InputEvent::MouseClick(campaign.ulx + 4, campaign.uly + 4),
    );

    let Screen::MissionSelect(form) = &screen else {
        panic!("expected MissionSelect");
    };
    assert_eq!(form.mission_count(None), 0);
    assert!(form.description_lines(None)[0].contains("unavailable"));

    assert_eq!(
        screen.handle_input(&mut ctx, InputEvent::Key(Key::Enter)),
        Transition::None
    );
    assert!(ctx.status.contains("registry"), "status: {}", ctx.status);
}

// ---------------------------------------------------------------------------
// Gates 1–2: the UI is data only
// ---------------------------------------------------------------------------

/// Owner gates 1–2: the new UI modules do not contain any direct mutation of the
/// application/universe — no `universe_mut`, no `*_mut` accessor, no `set_*` writer.
/// The start path of the menu is [`Transition::StartMission`] (checked above), which
/// `main.rs` executes through `App::start_mission_from_registry`.
#[test]
fn new_ui_code_never_mutates_the_application_directly() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    for module in ["newgame.rs", "gamesetupmenu.rs"] {
        let path = src.join(module);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));

        for forbidden in ["universe_mut", "_mut(", "set_"] {
            assert!(
                !source.contains(forbidden),
                "{module} contains the forbidden mutation token {forbidden:?}"
            );
        }
    }

    // The two screen handlers live in `ui.rs`; their block ends at the render method.
    let ui = std::fs::read_to_string(src.join("ui.rs")).expect("ui.rs");
    let start = ui
        .find("    fn handle_new_game_menu(")
        .expect("handle_new_game_menu");
    let end = ui
        .find("    /// Draws the screen into the frame.")
        .expect("the render method follows the menu handlers");
    let handlers = &ui[start..end];

    for forbidden in ["universe_mut", "_mut(", "set_"] {
        assert!(
            !handlers.contains(forbidden),
            "the New Game/GameSetupMenu handlers contain {forbidden:?}"
        );
    }
}

/// The mission choice is a pure DTO: category + index, no match state.
#[test]
fn mission_start_request_carries_only_the_choice() {
    let dir = mission_data_dir("dto");
    let registry = Arc::new(load_registry(&dir));
    let mut ui = MenuUi::new(registry);
    ui.open_category(NewGameAction::Campaign);

    let transition = ui.key(Key::Enter);
    match transition {
        Transition::StartMission(request) => {
            assert_eq!(request.category, MissionCategory::Campaign);
            assert_eq!(request.index, 0);
        }
        other => panic!("expected StartMission, got {other:?}"),
    }

    std::fs::remove_dir_all(&dir).ok();
}
