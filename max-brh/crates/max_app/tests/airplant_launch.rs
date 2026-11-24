//! AIRPLT: bringing out a built aircraft by a player order.
//!
//! Oracle:
//! * `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3489-3520`) — the unit is born on
//!   the factory cell in `ORDER_IDLE`/`ORDER_STATE_STORE`, `parent` — the factory,
//!   `scaler_adjust = 4`, the factory's `parent` — the unit and `storage = 1`
//!   (checked in `max-core/tests/production.rs`);
//! * `UnitsManager_Popup_InitFactories` (`units_manager.cpp:869-872`):
//!   a repeated click on a factory in `UNIT_READY` does **not** open the menu but selects
//!   the ready unit (`PlaceNewUnit` → `GameManager_EnableMainMenu(parent)`);
//! * `GameManager_GetAirUnitCursor` (`game_manager.cpp:5937-5962`) +
//!   `GameManager_ManagePlayerAction` (`:6918-6926`): a click on a cell next to
//!   the factory — `CURSOR_ACTIVATE`, the `ORDER_ACTIVATE` order goes to the **factory**;
//! * `UnitsManager_ActivateUnit` (`units_manager.cpp:3826-3913`): the aircraft
//!   gets `ORDER_MOVE`/`INIT` and `ScaleUnit(EXPAND)`, then takes off
//!   (`ExecuteMove` → `ORDER_TAKE_OFF`), the factory gives `--storage` and starts
//!   the next order.

use max_core::enums::{unit_flags, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::universe::{GameUniverse, LocalDispatcher, StateRef};
use max_core::team::{TeamId, TeamKind, TeamUnits};

use max_app::hud;
use max_app::{App, AppConfig};

fn state() -> GameUniverse<LocalDispatcher> {
    let mut state = GameUniverse::default();
    state
        .team_mut(TeamId::RED)
        .expect("RED slot")
        .team_type = TeamKind::Player;
    state
        .team_mut(TeamId::GREEN)
        .expect("GREEN slot")
        .team_type = TeamKind::Computer;
    *state.team_units_mut(TeamId::RED).expect("RED economy") =
        TeamUnits::with_clan(max_core::TeamClan::Random);
    *state.team_units_mut(TeamId::GREEN).expect("GREEN economy") =
        TeamUnits::with_clan(max_core::TeamClan::Random);
    state.set_active_turn_team(TeamId::RED);
    state.set_player_team(TeamId::RED);
    state
}

/// AIRPLT at `(10,10)` with a ready fighter "inside" — the state after
/// `UnitInfo::SpawnNewUnit`.
fn plant_with_fighter(state: &mut GameUniverse<LocalDispatcher>) -> (max_core::UnitId, max_core::UnitId) {
    let plant = state
        .spawn_unit(UnitType::AirUnitsPlant, TeamId::RED, Point::new(10, 10), 0)
        .expect("air plant");
    let fighter = state
        .spawn_unit(UnitType::Fighter, TeamId::RED, Point::new(10, 10), 0)
        .expect("fighter");

    {
        let mut unit = state.snapshot().unit(fighter).expect("fighter").clone();
        unit.parent_unit = Some(plant);
        unit.orders = UnitOrder::Idle;
        unit.state = UnitOrderState::Store;
        unit.scaler_adjust = 4;
        state.write_unit(fighter, unit);
    }
    {
        let mut unit = state.snapshot().unit(plant).expect("plant").clone();
        unit.parent_unit = Some(fighter);
        unit.storage = 1;
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::UnitReady;
        unit.build_time = 0;
        state.write_unit(plant, unit);
    }

    (plant, fighter)
}

#[test]
fn activate_order_targets_the_plant_and_carries_the_fighter_as_parent() {
    let mut universe = state();
    let (plant, fighter) = plant_with_fighter(&mut universe);

    // A cell next to the factory (2×2 at (10,10)-(11,11)) — `9,10`.
    let command = hud::build_activate_order(universe.snapshot(), fighter, Point::new(9, 10))
        .expect("CURSOR_ACTIVATE: приказ вывода");

    match command {
        max_core::input::Command::Order(packet) => {
            assert_eq!(packet.unit, plant, "приказ получает завод");
            match packet.payload {
                max_core::input::OrderPayload::P3(p3) => {
                    assert_eq!(p3.p2.p1.order, UnitOrder::Activate);
                    assert_eq!(p3.p2.p1.state, UnitOrderState::ExecutingOrder);
                    assert_eq!(p3.p2.parent_unit, Some(fighter), "клиент — самолёт");
                    assert_eq!(p3.move_to, Point::new(9, 10));
                }
                other => panic!("ожидался P3, получен {other:?}"),
            }
        }
        other => panic!("ожидался Order, получен {other:?}"),
    }
}

#[test]
fn clicking_the_plant_footprint_does_not_launch() {
    let mut universe = state();
    let (_plant, fighter) = plant_with_fighter(&mut universe);

    // Inside the factory 2×2 C++ gives `CURSOR_FRIEND` — no order.
    assert!(hud::build_activate_order(universe.snapshot(), fighter, Point::new(10, 10)).is_none());
    assert!(hud::build_activate_order(universe.snapshot(), fighter, Point::new(11, 11)).is_none());

    // Too far — not "next to the container".
    assert!(hud::build_activate_order(universe.snapshot(), fighter, Point::new(20, 20)).is_none());
}

#[test]
fn ordinary_units_have_no_activate_order() {
    let mut state = state();
    let tank = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            Point::new(5, 5),
            unit_flags::MOBILE_LAND_UNIT | unit_flags::SELECTABLE,
        )
        .expect("tank");

    assert!(hud::build_activate_order(StateRef::from(&state), tank, Point::new(6, 5)).is_none());
}

#[test]
fn air_move_order_is_a_packet_08_move_with_init_state() {
    let mut state = state();
    let fighter = state
        .spawn_unit(
            UnitType::Fighter,
            TeamId::RED,
            Point::new(10, 10),
            unit_flags::MOBILE_AIR_UNIT | unit_flags::SELECTABLE,
        )
        .expect("fighter");

    let command =
        hud::build_air_move_order(StateRef::from(&state), fighter, Point::new(13, 14)).expect("air move");
    match command {
        max_core::input::Command::Order(packet) => {
            assert_eq!(packet.unit, fighter);
            match packet.payload {
                max_core::input::OrderPayload::P3(p3) => {
                    assert_eq!(p3.p2.p1.order, UnitOrder::Move);
                    assert_eq!(p3.p2.p1.state, UnitOrderState::Init);
                    assert_eq!(p3.move_to, Point::new(13, 14));
                }
                other => panic!("ожидался P3, получен {other:?}"),
            }
        }
        other => panic!("ожидался Order, получен {other:?}"),
    }

    // A ground unit is not given an air order.
    let tank = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            Point::new(5, 5),
            unit_flags::MOBILE_LAND_UNIT,
        )
        .expect("tank");
    assert!(hud::build_air_move_order(StateRef::from(&state), tank, Point::new(6, 5)).is_none());
}

#[test]
fn app_launches_the_fighter_out_of_the_air_plant() {
    let mut app = App::new(AppConfig::default());
    app.adopt_universe(state());
    let (_plant, fighter) = plant_with_fighter(app.universe_mut());

    // Player: the ready unit is selected (after `PlaceNewUnit`), a click next to the factory.
    app.hud_mut().select(Some(fighter));
    let command = hud::build_activate_order(app.state_ref(), fighter, Point::new(9, 10))
        .expect("приказ вывода");
    assert_eq!(
        app.universe_mut().apply_now(command),
        max_core::ApplyOutcome::Applied,
        "08/P3 применён"
    );

    for _ in 0..64 {
        app.tick();

        let Some(unit) = app.state_ref().unit(fighter) else {
            break;
        };
        if unit.flags & unit_flags::HOVERING != 0 && unit.grid != Point::new(10, 10) {
            break;
        }
    }

    let unit = app.state_ref().unit(fighter).expect("fighter alive");
    assert_ne!(unit.grid, Point::new(10, 10), "самолёт вылетел из завода");
    assert!(
        unit.flags & unit_flags::HOVERING != 0,
        "самолёт в воздухе (`ORDER_TAKE_OFF`)"
    );
    assert!(unit.parent_unit.is_none(), "самолёт больше не в заводе");

    // `UnitInfo::Redraw` (`unitinfo.cpp:1452-1459`): a hovering aircraft draws its
    // shadow one cell down-right of the hull — the airborne look (defect 1 of the
    // manual 1.2 run: aircraft were drawn like parked ground units).
    let presentation = app
        .state_ref()
        .presentation(fighter)
        .expect("presentation of the fighter");
    assert_eq!(
        presentation.shadow_offset,
        Point::new(-64, -64),
        "hovering aircraft: `shadow_offset = (-GFX_MAP_TILE_SIZE, -GFX_MAP_TILE_SIZE)`"
    );
}

#[test]
fn unit_menu_on_a_ready_plant_selects_the_stored_fighter() {
    let mut app = App::new(AppConfig::default());
    app.adopt_universe(state());
    let (plant, fighter) = plant_with_fighter(app.universe_mut());

    // The first click on the factory — selection.
    app.hud_mut().select(Some(plant));
    // A repeated click (command menu) — `Popup_InitFactories` → `PlaceNewUnit`.
    assert!(app.open_unit_menu(), "клик по заводу обработан");
    assert!(app.unit_menu().is_none(), "меню не открывается");
    assert_eq!(
        app.hud().selected,
        Some(fighter),
        "выбран готовый юнит в заводе"
    );
    assert!(
        app.hud()
            .log
            .lines()
            .iter()
            .any(|line| line.contains("Unit ready in") && line.contains("launch")),
        "подсказка игроку: {:#?}",
        app.hud().log.lines()
    );
}
