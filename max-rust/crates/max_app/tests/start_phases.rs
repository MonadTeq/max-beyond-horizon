//! Тесты фаз старта (M-SP): мобилизация и выбор точки высадки.
//!
//! Проверяется то, что обещает задача B:
//!
//! * выбранная точка реально двигает базу (`deploy_starting_units_with`
//!   получает `TeamDeployment.position` и `FindValidStartingPosition`-совместима);
//! * покупка юнитов меняет состав (`TeamMissionSupplies::units`) и кредиты
//!   (`TeamUnits::SetGold`) ровно по формулам `CargoMenu`;
//! * порядок фаз hot-seat — как в C++ (`for (i = MAX-1; i >= RED; --i)`);
//! * ИИ выбирает свою точку до генерации ресурсов (`Ai_SelectStartingPosition`).

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
    app.state()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state().unit(id))
        .filter(|unit| unit.team == team)
        .map(|unit| unit.unit_type)
        .collect()
}

fn find_unit(app: &App, team: TeamId, unit_type: UnitType) -> Option<Point> {
    app.state()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state().unit(id))
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

    // «FindValidStartingPosition-совместимо»: точка — 4×4 чистой суши.
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
        app.state().team_units[TeamId::RED.0 as usize].gold,
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
        app.state()
            .unit_ids_in_order()
            .into_iter()
            .any(|id| app.state().unit(id).is_some_and(|unit| unit.team == TeamId::GREEN)),
        "у ИИ есть юниты"
    );
}

#[test]
fn landing_surface_is_loaded_from_wrl_only_when_present() {
    // `LandingScreen::load_surface` — поверхность планеты из `*.WRL`; без
    // данных игры фаза честно пропускается.
    let missing = max_app::landing::LandingScreen::load_surface(0, std::path::Path::new("."));
    assert!(missing.is_none(), "в текущем каталоге нет SNOW_1.WRL");

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let data = root.join("max-original");

    if !data.join("SNOW_1.WRL").is_file() {
        return;
    }

    let surface = max_app::landing::LandingScreen::load_surface(0, &data)
        .expect("SNOW_1.WRL читается");
    let landing = max_app::landing::LandingScreen::new(TeamId::RED, 0, surface);

    assert!(landing.cursor_valid(), "стартовый курсор — валидная точка");
}

/// Партия с фазами старта детерминирована: ИИ-точки и карта зависят только от
/// seed и выбранных людьми точек (C++ пересеивает ГПСЧ перед генерацией карты).
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
        app.state().team_units[TeamId::RED.0 as usize].gold,
        config.start_gold
    );
}
