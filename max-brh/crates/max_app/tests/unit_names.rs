//! Unit instance names (`UnitInfo::GetName` / `GetDisplayName`) and the
//! in-place rename — end-to-end through the application API.
//!
//! Oracle anchors:
//! * `units_manager.cpp:2377-2379` — `unit->unit_id = unit_counters[type];
//!   ++unit_counters[type]` at deployment;
//! * `unitinfo.cpp:757-779`/`:781-803` — `GetName` (`"Scout 7"`) and
//!   `GetDisplayName` (`"Mk II Scout 7"`);
//! * `buildmenu.cpp:944-946` — `"%s %i will be available in %i turns."` with the
//!   counter read **before** the deployment increments it;
//! * `game_manager.cpp:3993-4023`/`:5371-5381` — the corner-portrait `TextEdit`
//!   and its commit as packet **43**.
//!
//! The tests need real assets (`MAX.RES`, `SNOW_1.WRL`) next to the repository —
//! like `unit_menu.rs`; without them they skip.

use std::path::{Path, PathBuf};

use max_app::local::LocalSetup;
use max_app::pregame::{PreGameFlow, PreGameStage};
use max_app::setup::GameSetupForm;
use max_app::ui::{handle_session_input, InputEvent, Key};
use max_app::{App, AppConfig, SessionChannel};
use max_core::enums::UnitType;
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitId;
use max_core::UnitTypeClassify;
use max_render::layout;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

/// One human RED team, the rest — AI/empty (the corner portrait rename needs
/// `TEAM_TYPE_PLAYER`).
fn single_human() -> LocalSetup {
    let mut setup = LocalSetup::default();
    setup.world = 0;
    setup.team_types = [
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::None,
        TeamKind::None,
    ];
    setup
}

fn app_with_assets() -> Option<App> {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");
    let patches = repo_path("tests/fixtures/assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("unit_names: нет данных игры рядом с репозиторием");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("tests/fixtures/assets"));
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");

    Some(app)
}

/// A match through the real start phases: mobilization → landing → `InGame`.
fn game_app() -> Option<App> {
    let world_dir = repo_path("max-original");

    if !world_dir.join("SNOW_1.WRL").is_file() {
        eprintln!("unit_names: нет данных — нет SNOW_1.WRL");
        return None;
    }

    let mut flow = PreGameFlow::start(GameSetupForm::default(), single_human(), &world_dir);
    assert!(matches!(flow.stage, PreGameStage::Mobilize(_)));
    flow.commit_mobilization();
    assert!(matches!(flow.stage, PreGameStage::Landing(_)));
    flow.commit_landing();

    let config = flow.setup.new_game_config();
    let surface =
        max_app::landing::LandingScreen::load_surface(0, &world_dir).expect("WRL-поверхность");

    let mut app = app_with_assets()?;
    app.new_game_on_surface(&config, &surface);
    app.tick();

    Some(app)
}

/// A free land cell next to the RED mine.
fn free_grid(app: &App) -> Point {
    for radius in 1..8 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let grid = Point::new(22 + dx, 22 + dy);

                if app.state_ref().units_at(grid).is_empty()
                    && app.state_ref().map().surface_at(grid.x, grid.y).bits()
                        & max_core::enums::Surface::LAND.bits()
                        != 0
                {
                    return grid;
                }
            }
        }
    }

    Point::new(23, 23)
}

/// Adds a RED unit to a free cell.
fn spawn(app: &mut App, unit_type: UnitType) -> UnitId {
    let grid = free_grid(app);
    let flags = max_core::access::unit_type_flags(unit_type);
    let id = app
        .universe_mut()
        .spawn_unit(unit_type, TeamId::RED, grid, flags)
        .expect("юнит появился");

    app.viewport_mut().center_on_grid(grid.x, grid.y);
    id
}

/// The team's deployment counter for a type (read view).
fn team_counter(app: &App, unit_type: UnitType) -> u32 {
    app.state_ref().teams()[TeamId::RED.0 as usize].team_unit_counter(unit_type as u16)
}

/// Click on the corner portrait — the C++ `Gamemanager_FlicButton` `GNW` button.
fn click_corner_portrait(app: &mut App) {
    let area = layout::CORNER_FLIC;
    let _ = handle_session_input(app, InputEvent::MouseClick(area.ulx + 4, area.uly + 4));
}

fn type_text(app: &mut App, text: &str) {
    for character in text.chars() {
        let _ = handle_session_input(app, InputEvent::Key(Key::Char(character)));
    }
}

#[test]
fn deployment_assigns_the_instance_number_from_the_team_counter() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));

    let first = spawn(&mut app, UnitType::Tank);
    let second = spawn(&mut app, UnitType::Tank);

    // `CTInfo::Reset` starts at 1, `DeployUnit` takes the counter and advances it.
    assert_eq!(app.state_ref().unit(first).expect("tank").unit_id, 1);
    assert_eq!(app.state_ref().unit(second).expect("tank").unit_id, 2);
    assert_eq!(team_counter(&app, UnitType::Tank), 3);

    // Another type has its own counter.
    let scout = spawn(&mut app, UnitType::Scout);
    assert_eq!(app.state_ref().unit(scout).expect("scout").unit_id, 1);
    assert_eq!(team_counter(&app, UnitType::Scout), 2);
}

#[test]
fn corner_portrait_click_renames_through_packet_43() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));
    assert!(app.chrome_active(), "1:1 window chrome");

    let id = spawn(&mut app, UnitType::Tank);
    app.hud_mut().select(Some(id));

    click_corner_portrait(&mut app);
    let editor = app.unit_rename().expect("поле переименования открыто");
    let unit_id = app.state_ref().unit(id).expect("tank").unit_id;
    assert_eq!(
        editor.text,
        format!("Tank {unit_id}"),
        "поле начинается с GetName (unitinfo.cpp:4001)"
    );

    // The first character replaces the selected `GetName`
    // (`EnterTextEditField` selects the whole text).
    type_text(&mut app, "Oddball");
    assert_eq!(app.unit_rename().expect("поле").text, "Oddball");

    // `Enter` — `UnitInfo::SetName` + packet 43 through the command channel.
    let _ = handle_session_input(&mut app, InputEvent::Key(Key::Enter));
    assert!(app.unit_rename().is_none(), "Enter закрывает поле");
    assert_eq!(
        app.state_ref().unit(id).expect("tank").name.as_deref(),
        Some("Oddball")
    );

    // The corner portrait and the statistics window now show `Mk <roman> <name>`.
    assert_eq!(
        max_app::panels::unit_display_name(app.state_ref().unit(id).expect("tank")),
        "Mk I Oddball"
    );
    assert!(app.open_unit_stats(id));
    assert_eq!(
        app.unit_stats().expect("UnitStats").name,
        "Mk I Oddball",
        "заголовок UnitStats — GetDisplayName (unitstats.cpp:520)"
    );
}

#[test]
fn escape_cancels_the_rename() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));

    let id = spawn(&mut app, UnitType::Tank);
    app.hud_mut().select(Some(id));

    click_corner_portrait(&mut app);
    assert!(app.unit_rename().is_some());

    type_text(&mut app, "Oddball");
    let _ = handle_session_input(&mut app, InputEvent::Key(Key::Escape));
    assert!(app.unit_rename().is_none(), "Esc закрывает поле");

    assert_eq!(app.state_ref().unit(id).expect("tank").name, None);
}

#[test]
fn empty_name_clears_the_own_name() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));

    let id = spawn(&mut app, UnitType::Tank);
    app.hud_mut().select(Some(id));

    // First name: `Oddball`.
    click_corner_portrait(&mut app);
    type_text(&mut app, "Oddball");
    let _ = handle_session_input(&mut app, InputEvent::Key(Key::Enter));
    assert_eq!(
        app.state_ref().unit(id).expect("tank").name.as_deref(),
        Some("Oddball")
    );

    // Second open: the field starts with the user name, Backspace clears the
    // selection, Enter commits an empty string → `SetName("")` → `nullptr`.
    click_corner_portrait(&mut app);
    assert_eq!(app.unit_rename().expect("поле").text, "Oddball");
    let _ = handle_session_input(&mut app, InputEvent::Key(Key::Backspace));
    assert_eq!(app.unit_rename().expect("поле").text, "");
    let _ = handle_session_input(&mut app, InputEvent::Key(Key::Enter));

    assert_eq!(app.state_ref().unit(id).expect("tank").name, None);
    let unit_id = app.state_ref().unit(id).expect("tank").unit_id;
    assert_eq!(
        max_app::panels::unit_name(app.state_ref().unit(id).expect("tank")),
        format!("Tank {unit_id}"),
        "после очистки имени — снова GetName"
    );
}

#[test]
fn foreign_unit_portrait_does_not_open_the_rename() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));

    // A GREEN (computer) unit: the C++ FLIC button is disabled for it.
    let grid = free_grid(&app);
    let flags = max_core::access::unit_type_flags(UnitType::Tank);
    let enemy = app
        .universe_mut()
        .spawn_unit(UnitType::Tank, TeamId::GREEN, grid, flags)
        .expect("enemy tank");
    app.hud_mut().select(Some(enemy));

    click_corner_portrait(&mut app);
    assert!(
        app.unit_rename().is_none(),
        "чужой юнит не переименовывается (game_manager.cpp:7140)"
    );
}

#[test]
fn build_message_names_the_counter_before_the_deployment() {
    let mut app = game_app().expect(concat!("test fixture is missing: ", stringify!(game_app())));

    // A mobile builder with enough storage for a 1× order; the engineer's list is
    // the one with non-building (1×1) entries, i.e. the C++ direct-commit branch.
    let builder = spawn(&mut app, UnitType::Engineer);
    app.universe_mut()
        .unit_mut(builder)
        .expect("engineer")
        .storage = 1000;
    app.hud_mut().select(Some(builder));
    assert!(app.open_build_menu(), "BuildMenu открыт");

    let list = max_core::production::buildable_units(UnitType::Engineer);
    let index = list
        .iter()
        .position(|unit_type| !unit_type.is_quad())
        .expect("у инженера есть нес-здание в списке");
    let head = list[index];
    assert!(app.build_menu_select(index));

    let build_rate = app.build_menu().expect("меню").build_rate;
    let counter = team_counter(&app, head);

    let line = app
        .commit_build_menu(SessionChannel::Local)
        .expect("Done обработан");

    // `MobileBuildMenu::Build` (`buildmenu.cpp:944-946`):
    // `"%s %i will be available in %i turns."`.
    let state = app.state_ref();
    let (turns, _) = state.turns_and_consumption(
        state.unit(builder).expect("engineer"),
        head,
        i32::from(build_rate.max(1)),
    );
    assert_eq!(
        line,
        format!(
            "{} {counter} will be available in {turns} turns.",
            max_app::buildmenu::unit_name(head)
        )
    );

    // The order itself does not advance the counter — only `DeployUnit` does.
    assert_eq!(
        team_counter(&app, head),
        counter,
        "счётчик читается ДО инкремента"
    );

    // The produced instance gets that number, and the counter advances.
    let site = free_grid(&app);
    let flags = max_core::access::unit_type_flags(head);
    let deployed = app
        .universe_mut()
        .spawn_unit(head, TeamId::RED, site, flags)
        .expect("deployed unit");
    assert_eq!(
        app.state_ref().unit(deployed).expect("unit").unit_id,
        counter as u8,
        "номер экземпляра = счётчик из сообщения"
    );
    assert_eq!(team_counter(&app, head), counter + 1);
}
