//! Start phase tests (M-SP): mobilization and landing point selection.
//!
//! What task B promises is checked:
//!
//! * the chosen point really moves the base (`deploy_starting_units_with`
//!   receives `TeamDeployment.position` and is `FindValidStartingPosition`-compatible);
//! * buying units changes the composition (`TeamMissionSupplies::units`) and credits
//!   (`TeamUnits::SetGold`) exactly by the `CargoMenu` formulas;
//! * the hot-seat phase order — as in C++ (`for (i = MAX-1; i >= RED; --i)`);
//! * the AI chooses its point before resource generation (`Ai_SelectStartingPosition`).

use max_app::local::{LocalSetup, START_POSITIONS};
use max_app::mobilize::{unit_cost, Mobilization, PURCHASE_UNITS};
use max_app::pregame::{phase_plan, PreGameFlow, PreGameStep};
use max_app::{App, AppConfig, NewGameConfig};
use max_core::deploy::DEFAULT_MISSION_LOADOUT;
use max_core::enums::{Surface, TeamClan, UnitType};
use max_core::geo::{Point, MAP_TILES};
use max_core::map::SurfaceMap;
use max_core::setup::is_valid_starting_position;
use max_core::team::{TeamId, TeamKind};

fn land_surface() -> SurfaceMap {
    SurfaceMap::from_tiles([Surface::LAND; MAP_TILES])
}

fn app() -> App {
    App::new(AppConfig::default())
}

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

fn team_units(app: &App, team: TeamId) -> Vec<UnitType> {
    app.state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state_ref().unit(id))
        .filter(|unit| unit.team == team)
        .map(|unit| unit.unit_type)
        .collect()
}

fn find_unit(app: &App, team: TeamId, unit_type: UnitType) -> Option<Point> {
    app.state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state_ref().unit(id))
        .find(|unit| unit.team == team && unit.unit_type == unit_type)
        .map(|unit| unit.grid)
}

#[test]
fn landing_position_moves_the_base() {
    let chosen = Point::new(60, 60);
    let mut setup = single_human();
    setup.start_positions[0] = Some(chosen);

    let config = setup.new_game_config();

    assert_eq!(config.deployments.len(), 2, "одна запись на играющую команду");
    assert_eq!(config.deployments[0].position, chosen);

    let mut app = app();
    app.new_game_on_surface(&config, &land_surface());

    let base = find_unit(&app, TeamId::RED, UnitType::MiningStation).expect("шахта высажена");
    let dx = i32::from(base.x) - i32::from(chosen.x);
    let dy = i32::from(base.y) - i32::from(chosen.y);

    assert!(
        dx.abs() <= 12 && dy.abs() <= 12,
        "база должна стоять у выбранной точки ({chosen:?}), а не {base:?}"
    );

    // "FindValidStartingPosition-compatible": the point is 4×4 of pure land.
    assert!(is_valid_starting_position(
        &land_surface(),
        i32::from(base.x),
        i32::from(base.y)
    ));
}

#[test]
fn chosen_position_differs_from_the_default() {
    let mut setup = single_human();
    setup.start_positions[0] = Some(Point::new(60, 60));

    let config = setup.new_game_config();
    assert_ne!(
        config.teams[0].position, START_POSITIONS[0],
        "точка из фазы высадки должна заменить стартовую четверть"
    );

    setup.start_positions[0] = None;
    let config = setup.new_game_config();

    assert_eq!(config.teams[0].position, START_POSITIONS[0]);
    assert!(
        config.deployments.is_empty(),
        "без фаз — старый путь без TeamDeployment"
    );
}

#[test]
fn mobilization_buys_units_and_spends_credits() {
    let mut setup = single_human();
    setup.start_positions[0] = Some(Point::new(40, 40));

    let mut mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, setup.start_gold);
    assert!(mobilization.buy(0), "Constructor");
    assert!(mobilization.buy(0), "Constructor");

    let spent = 2 * unit_cost(UnitType::Constructor, TeamClan::Random);
    let remaining = setup.start_gold - spent;

    setup.team_gold[0] = Some(mobilization.credits);
    setup.team_loadouts[0] = Some(mobilization.loadout());

    assert_eq!(setup.team_gold[0], Some(remaining));
    assert_eq!(
        setup.team_loadouts[0].as_ref().unwrap().len(),
        DEFAULT_MISSION_LOADOUT.len() + 2
    );

    let config = setup.new_game_config();
    let deployment = &config.deployments[0];

    assert_eq!(deployment.gold, remaining);
    assert_eq!(deployment.supplies.len(), DEFAULT_MISSION_LOADOUT.len() + 2);

    let mut app = app();
    app.new_game_on_surface(&config, &land_surface());

    assert_eq!(
        app.state_ref().team_units()[TeamId::RED.0 as usize].gold,
        remaining,
        "team_gold после закупки (TeamUnits::SetGold)"
    );

    let constructors = team_units(&app, TeamId::RED)
        .into_iter()
        .filter(|unit| *unit == UnitType::Constructor)
        .count();

    assert_eq!(constructors, 3, "один бесплатный + два купленных");
}

#[test]
fn purchase_list_matches_cargo_menu_and_costs_are_cpp_exact() {
    assert_eq!(PURCHASE_UNITS.len(), 15);

    for unit in PURCHASE_UNITS {
        let expected = 3 * max_core::unit_values::team_base_values(TeamClan::Random)
            [unit as usize]
            .get_attribute(max_core::unit_values::ATTRIB_TURNS)
            .max(0) as u32;

        assert_eq!(unit_cost(unit, TeamClan::Random), expected);
    }
}

#[test]
fn hot_seat_phases_run_in_reverse_team_order() {
    let mut setup = LocalSetup::default();
    setup.hot_seat = true;
    setup.team_types = [
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::None,
        TeamKind::None,
    ];

    assert_eq!(
        phase_plan(&setup),
        vec![
            PreGameStep::Mobilize(TeamId::GREEN),
            PreGameStep::Landing(TeamId::GREEN),
            PreGameStep::Mobilize(TeamId::RED),
            PreGameStep::Landing(TeamId::RED),
        ],
        "C++: for (i = PLAYER_TEAM_MAX - 1; i >= PLAYER_TEAM_RED; --i)"
    );

    let flow = PreGameFlow::start(
        max_app::setup::GameSetupForm::default(),
        setup,
        std::path::Path::new("."),
    );

    assert_eq!(flow.current_team(), Some(TeamId::GREEN));
}

#[test]
fn zero_start_gold_skips_mobilization() {
    let mut setup = single_human();
    setup.start_gold = 0;

    assert_eq!(
        phase_plan(&setup),
        vec![PreGameStep::Landing(TeamId::RED)],
        "C++: CargoMenu открывается только при team_gold != 0"
    );
}

#[test]
fn ai_teams_get_their_own_starting_positions() {
    let mut setup = single_human();
    setup.start_positions[0] = Some(Point::new(60, 60));
    let config = setup.new_game_config();

    let mut app = app();
    app.new_game_on_surface(&config, &land_surface());

    let ai_base = find_unit(&app, TeamId::GREEN, UnitType::MiningStation);

    assert!(ai_base.is_some(), "ИИ-команда высажена на своей точке");
    assert!(
        app.state_ref()
            .unit_ids_in_order()
            .into_iter()
            .any(|id| app.state_ref().unit(id).is_some_and(|unit| unit.team == TeamId::GREEN)),
        "у ИИ есть юниты"
    );
}

#[test]
fn landing_surface_is_loaded_from_wrl_only_when_present() {
    // `LandingScreen::load_surface` — the planet surface from `*.WRL`; without
    // game data the phase is honestly skipped.
    let missing = max_app::landing::LandingScreen::load_surface(0, std::path::Path::new("."));
    assert!(missing.is_none(), "в текущем каталоге нет SNOW_1.WRL");

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let data = root.join("max-original");

    assert!(
        data.join("SNOW_1.WRL").is_file(),
        "SNOW_1.WRL is required for the second half of this test"
    );

    let surface = max_app::landing::LandingScreen::load_surface(0, &data)
        .expect("SNOW_1.WRL читается");
    let landing = max_app::landing::LandingScreen::new(TeamId::RED, 0, surface);

    assert!(landing.cursor_valid(), "стартовый курсор — валидная точка");
}

/// A match with start phases is deterministic: the AI points and the map depend only on
/// the seed and the points chosen by humans (C++ reseeds the RNG before map generation).
#[test]
fn phased_start_is_deterministic() {
    let mut setup = single_human();
    setup.start_positions[0] = Some(Point::new(60, 60));
    let config = setup.new_game_config();

    let mut first = app();
    first.new_game_on_surface(&config, &land_surface());

    let mut second = app();
    second.new_game_on_surface(&config, &land_surface());

    assert_eq!(first.snapshot(), second.snapshot());
}

#[test]
fn default_config_keeps_the_old_behavior() {
    let config = NewGameConfig::default();
    assert!(config.deployments.is_empty());

    let mut app = app();
    let report = app.new_game_on_surface(&config, &land_surface());

    assert_eq!(report.start_units, 14, "2 команды × (4 постройки + 3 юнита)");
    assert_eq!(
        app.state_ref().team_units()[TeamId::RED.0 as usize].gold,
        config.start_gold
    );
}

/// `CargoMenu` upgrade: an attribute upgrade before landing changes
/// `TeamUnits.current_values`, so the starting units of the type get the new
/// `UnitValues` (`GameManager_CargoSelection` → `CommitUpgradeChanges`).
#[test]
fn mobilization_upgrades_reach_the_deployed_units() {
    let mut setup = single_human();
    setup.start_positions[0] = Some(Point::new(40, 40));

    let mut mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, setup.start_gold);
    mobilization.select_loadout(0); // free Constructor — row 0
    assert!(mobilization.buy(0), "второй Constructor — проверяем оба");

    let base_armor = mobilization.base[UnitType::Constructor as usize].armor;
    assert!(mobilization.adjust_upgrade(0, true), "armor +1");
    assert_eq!(
        mobilization.actual[UnitType::Constructor as usize].armor,
        base_armor + 1
    );

    let upgrades = mobilization.changed_upgrades();
    assert_eq!(upgrades.len(), 1);
    assert_eq!(upgrades[0].0, UnitType::Constructor);

    setup.team_gold[0] = Some(mobilization.credits);
    setup.team_loadouts[0] = Some(mobilization.loadout());
    setup.team_upgrades[0] = Some(upgrades);
    setup.team_gold_spent[0] = Some(mobilization.gold_spent_on_upgrades());

    let config = setup.new_game_config();
    assert_eq!(config.deployments[0].upgrades.len(), 1);
    assert!(config.deployments[0].gold_spent_on_upgrades > 0);

    let mut app = app();
    app.new_game_on_surface(&config, &land_surface());

    let constructors: Vec<_> = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter(|id| {
            app.state_ref()
                .unit(*id)
                .is_some_and(|unit| unit.team == TeamId::RED && unit.unit_type == UnitType::Constructor)
        })
        .collect();

    assert_eq!(constructors.len(), 2, "бесплатный + купленный Constructor");
    for id in constructors {
        let values = app.state_ref().unit(id).unwrap().base_values;
        assert_eq!(
            values.armor,
            base_armor + 1,
            "апгрейд применяется ко всем юнитам типа"
        );
    }
    assert_eq!(
        app.state_ref().teams()[TeamId::RED.0 as usize].stats_gold_spent_on_upgrades,
        config.deployments[0].gold_spent_on_upgrades
    );
}
