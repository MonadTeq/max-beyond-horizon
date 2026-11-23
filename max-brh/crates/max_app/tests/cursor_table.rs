//! Cursor table — 1:1 `GameManager_GetWindowCursor` (`game_manager.cpp:4215`).
//!
//! Every mode from the oracle is checked:
//! * an empty map without selection → `UNIT_NGO` (`SelectedUnit == nullptr`);
//! * an own unit → `FRND_PTR`;
//! * X-FER (`unit->cursor = CURSOR_ARROW_NE`) → `FRND_XFR` over compatible
//!   cargo, otherwise `FRND_PTR` (`game_manager.cpp:6031-6047`);
//! * repair (`CURSOR_ARROW_E`) → `FRND_FIX` (`:6049-6056`);
//! * reload (`CURSOR_ARROW_S`) → `PTR_RLD` (`:6058-6065`);
//! * load (`CURSOR_ARROW_SW`) → `FRND_LOD` (`:5997-6021`);
//! * targeting_mode → `ENMY_PTR`/`UNIT_NGO` (`:4384-4414`);
//! * military cursor → `ENMY_PTR`/`PTR_FTRG`/`UNIT_NGO` (`:6150`);
//! * air → `UNIT_GO`, Shift → `WAY_PTR` (`:4441-4465`);
//! * a builder in `ORDER_BUILD` → `UNIT_NGO` (`:5927-5931`).

use max_app::cursor::{window_cursor, CursorId};
use max_app::{App, AppConfig};
use max_core::access;
use max_core::enums::{
    GameStateType, PlayMode, Surface, UnitOrder, UnitOrderState, UnitType,
};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::{TeamClan, UnitId};

fn app() -> App {
    let mut app = App::new(AppConfig::default());
    {
        let state = &mut app.universe;
        state.set_game_state(GameStateType::InGame);
        state.set_play_mode(PlayMode::TurnBased);
        state.set_player_team(TeamId::RED);
        state.set_active_turn_team(TeamId::RED);
        state.teams_mut()[0].team_type = TeamKind::Player;
        state.teams_mut()[1].team_type = TeamKind::Computer;
        state.team_units_table_mut()[0] = TeamUnits::with_clan(TeamClan::Random);
        state.team_units_table_mut()[1] = TeamUnits::with_clan(TeamClan::Random);
        state.ai_mut().all_visible = true;

        for tile in state.map_mut().surface.tiles_mut().iter_mut() {
            *tile = Surface::LAND;
        }
    }

    app
}

fn spawn(app: &mut App, unit_type: UnitType, team: TeamId, grid: Point) -> Option<UnitId> {
    let flags = access::unit_type_flags(unit_type);

    app.universe.spawn_unit(unit_type, team, grid, flags)
}

fn ready(app: &mut App, id: UnitId) {
    let mut unit = app.state_ref().unit(id).expect("unit").clone();
    unit.orders = UnitOrder::Await;
    unit.state = UnitOrderState::ExecutingOrder;
    unit.hits = unit
        .base_values
        .get_attribute(max_core::unit_values::ATTRIB_HITS)
        .max(1) as u16;
    unit.ammo = unit
        .base_values
        .get_attribute(max_core::unit_values::ATTRIB_AMMO)
        .max(1) as u8;
    app.universe.write_unit(id, unit);
}

#[test]
fn cursor_ids_match_the_cpp_enum() {
    assert_eq!(CursorId::Transfer as u8, 13, "CURSOR_TRANSFER");
    assert_eq!(CursorId::Repair as u8, 12, "CURSOR_REPAIR");
    assert_eq!(CursorId::Load as u8, 16, "CURSOR_LOAD");
    assert_eq!(CursorId::Reload as u8, 15, "CURSOR_RELOAD");
    assert_eq!(CursorId::Friend as u8, 17, "CURSOR_FRIEND");
    assert_eq!(CursorId::Enemy as u8, 18, "CURSOR_ENEMY");
    assert_eq!(CursorId::FarTarget as u8, 19, "CURSOR_FAR_TARGET");
    assert_eq!(CursorId::UnitGo as u8, 20, "CURSOR_UNIT_GO");
    assert_eq!(CursorId::UnitNoGo as u8, 21, "CURSOR_UNIT_NO_GO");
    assert_eq!(CursorId::Way as u8, 22, "CURSOR_WAY");
    assert_eq!(CursorId::Activate as u8, 24, "CURSOR_ACTIVATE");
    assert_eq!(CursorId::Map2 as u8, 25, "CURSOR_MAP2");
    assert_eq!(CursorId::Steal as u8, 26, "CURSOR_STEAL");
    assert_eq!(CursorId::Disable as u8, 27, "CURSOR_DISABLE");
}

#[test]
fn empty_map_without_selection_is_unit_no_go() {
    // C++ `GameManager_GetWindowCursor` has no `MAP_PTR` branch:
    // selection, an empty cell gives `CURSOR_UNIT_NO_GO` (`game_manager.cpp:4330-4335`).
    let app = app();

    assert_eq!(
        window_cursor(&app, Point::new(40, 40)),
        CursorId::UnitNoGo
    );
}

#[test]
fn own_unit_without_selection_is_friend() {
    let mut app = app();
    let tank = spawn(&mut app, UnitType::Tank, TeamId::RED, Point::new(40, 40)).expect("tank");
    ready(&mut app, tank);

    assert_eq!(window_cursor(&app, Point::new(40, 40)), CursorId::Friend);
}

#[test]
fn xfer_mode_over_matching_cargo_is_transfer() {
    let mut app = app();
    let constructor = spawn(
        &mut app,
        UnitType::Constructor,
        TeamId::RED,
        Point::new(40, 40),
    )
    .expect("ctor");
    let depot = spawn(
        &mut app,
        UnitType::MaterialStorage,
        TeamId::RED,
        Point::new(41, 40),
    )
    .expect("raw dump");
    ready(&mut app, constructor);
    ready(&mut app, depot);
    app.hud_mut().select(Some(constructor));
    app.hud_mut().cursor_mode = max_app::unitmenu::CURSOR_TRANSFER;

    assert_eq!(
        window_cursor(&app, Point::new(41, 40)),
        CursorId::Transfer,
        "X-FER: одинаковая категория груза → FRND_XFR"
    );
}

#[test]
fn xfer_mode_over_other_cargo_is_friend() {
    let mut app = app();
    let constructor =
        spawn(&mut app, UnitType::Constructor, TeamId::RED, Point::new(40, 40)).expect("ctor");
    let tank = spawn(&mut app, UnitType::Tank, TeamId::RED, Point::new(41, 40)).expect("tank");
    ready(&mut app, constructor);
    ready(&mut app, tank);
    app.hud_mut().select(Some(constructor));
    app.hud_mut().cursor_mode = max_app::unitmenu::CURSOR_TRANSFER;

    assert_eq!(
        window_cursor(&app, Point::new(41, 40)),
        CursorId::Friend,
        "танк не принимает сырьё — обычный FRND_PTR"
    );
}

#[test]
fn repair_mode_over_damaged_unit_is_repair() {
    let mut app = app();
    let engineer =
        spawn(&mut app, UnitType::Engineer, TeamId::RED, Point::new(40, 40)).expect("engineer");
    let tank = spawn(&mut app, UnitType::Tank, TeamId::RED, Point::new(41, 40)).expect("tank");
    ready(&mut app, engineer);
    ready(&mut app, tank);
    app.hud_mut().select(Some(engineer));
    app.hud_mut().cursor_mode = max_app::unitmenu::CURSOR_REPAIR;

    let damaged = {
        let mut unit = app.state_ref().unit(tank).expect("tank").clone();
        unit.hits = 1;
        app.universe.write_unit(tank, unit.clone());
        unit
    };

    assert_eq!(
        window_cursor(&app, Point::new(41, 40)),
        CursorId::Repair,
        "повреждённая цель → FRND_FIX"
    );

    let mut full = damaged;
    full.hits = full
        .base_values
        .get_attribute(max_core::unit_values::ATTRIB_HITS) as u16;
    app.universe.write_unit(tank, full);

    assert_eq!(
        window_cursor(&app, Point::new(41, 40)),
        CursorId::Friend,
        "целая цель — нечего чинить"
    );
}

#[test]
fn reload_mode_over_empty_ammo_is_reload() {
    let mut app = app();
    let depot = spawn(&mut app, UnitType::Depot, TeamId::RED, Point::new(40, 40)).expect("depot");
    // the 2×2 building occupies 40..41 on both axes, the tank — on the adjacent cell 42.
    let tank = spawn(&mut app, UnitType::Tank, TeamId::RED, Point::new(42, 40)).expect("tank");
    ready(&mut app, depot);
    ready(&mut app, tank);
    app.hud_mut().select(Some(depot));
    app.hud_mut().cursor_mode = max_app::unitmenu::CURSOR_RELOAD;

    {
        let mut unit = app.state_ref().unit(tank).expect("tank").clone();
        unit.ammo = 0;
        app.universe.write_unit(tank, unit);
    }

    assert_eq!(
        window_cursor(&app, Point::new(42, 40)),
        CursorId::Reload,
        "пустой боезапас → PTR_RLD"
    );
}

#[test]
fn load_mode_over_passenger_is_load() {
    let mut app = app();
    let carrier = spawn(
        &mut app,
        UnitType::ArmouredPersonnelCarrier,
        TeamId::RED,
        Point::new(40, 40),
    )
    .expect("carrier");
    let infantry =
        spawn(&mut app, UnitType::Infantry, TeamId::RED, Point::new(41, 40)).expect("infantry");
    ready(&mut app, carrier);
    ready(&mut app, infantry);
    app.hud_mut().select(Some(carrier));
    app.hud_mut().cursor_mode = max_app::unitmenu::CURSOR_LOAD;

    {
        let mut unit = app.state_ref().unit(carrier).expect("carrier").clone();
        unit.storage = 0;
        app.universe.write_unit(carrier, unit);
    }

    assert_eq!(
        window_cursor(&app, Point::new(41, 40)),
        CursorId::Load,
        "CLNTRANS + INFANTRY → FRND_LOD"
    );
}

#[test]
fn targeting_mode_in_and_out_of_range() {
    let mut app = app();
    let tank = spawn(&mut app, UnitType::Tank, TeamId::RED, Point::new(40, 40)).expect("tank");
    let enemy = spawn(&mut app, UnitType::Tank, TeamId::BLUE, Point::new(41, 40)).expect("enemy");
    ready(&mut app, tank);
    ready(&mut app, enemy);
    app.universe.update_visibility_status(true);
    app.hud_mut().select(Some(tank));
    app.hud_mut().targeting_mode = true;

    assert_eq!(
        window_cursor(&app, Point::new(41, 40)),
        CursorId::Enemy,
        "цель в радиусе → ENMY_PTR"
    );

    let far = Point::new(60, 40);

    assert_eq!(
        window_cursor(&app, far),
        CursorId::UnitNoGo,
        "вне радиуса → UNIT_NGO"
    );
}

#[test]
fn military_cursor_marks_far_targets() {
    let mut app = app();
    let tank = spawn(&mut app, UnitType::Tank, TeamId::RED, Point::new(40, 40)).expect("tank");
    let enemy = spawn(&mut app, UnitType::Tank, TeamId::BLUE, Point::new(50, 40)).expect("enemy");
    ready(&mut app, tank);
    ready(&mut app, enemy);
    app.universe.update_visibility_status(true);
    app.hud_mut().select(Some(tank));

    assert_eq!(
        window_cursor(&app, Point::new(50, 40)),
        CursorId::FarTarget,
        "цель вне радиуса и юнит мобильный → PTR_FTRG"
    );
}

#[test]
fn air_unit_cursor_is_unit_go_or_way() {
    let mut app = app();
    let plane = spawn(&mut app, UnitType::Fighter, TeamId::RED, Point::new(40, 40)).expect("plane");
    ready(&mut app, plane);
    app.hud_mut().select(Some(plane));

    assert_eq!(
        window_cursor(&app, Point::new(44, 44)),
        CursorId::UnitGo,
        "воздух над пустой клеткой → UNIT_GO"
    );

    app.set_shift_pressed(true);

    assert_eq!(
        window_cursor(&app, Point::new(44, 44)),
        CursorId::Way,
        "Shift → WAY_PTR (маршрут по точкам)"
    );
}

#[test]
fn builder_in_build_progress_is_unit_no_go() {
    let mut app = app();
    let constructor =
        spawn(&mut app, UnitType::Constructor, TeamId::RED, Point::new(40, 40)).expect("ctor");

    {
        let mut unit = app.state_ref().unit(constructor).expect("ctor").clone();
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.build_list = vec![UnitType::TrainingHall];
        unit.build_time = 5;
        app.universe.write_unit(constructor, unit);
    }
    app.hud_mut().select(Some(constructor));

    assert_eq!(
        window_cursor(&app, Point::new(45, 45)),
        CursorId::UnitNoGo,
        "GetBuilderUnitCursor вне UNIT_READY/SELECT_SITE → UNIT_NGO"
    );
}
