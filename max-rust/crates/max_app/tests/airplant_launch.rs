//! AIRPLT: вывод построенного самолёта приказом игрока.
//!
//! Оракул:
//! * `UnitInfo::SpawnNewUnit` (`unitinfo.cpp:3489-3520`) — юнит рождается на
//!   клетке завода в `ORDER_IDLE`/`ORDER_STATE_STORE`, `parent` — завод,
//!   `scaler_adjust = 4`, у завода `parent` — юнит и `storage = 1`
//!   (проверяется в `max-core/tests/production.rs`);
//! * `UnitsManager_Popup_InitFactories` (`units_manager.cpp:869-872`):
//!   повторный клик по заводу в `UNIT_READY` **не** открывает меню, а выбирает
//!   готового юнита (`PlaceNewUnit` → `GameManager_EnableMainMenu(parent)`);
//! * `GameManager_GetAirUnitCursor` (`game_manager.cpp:5937-5962`) +
//!   `GameManager_ManagePlayerAction` (`:6918-6926`): клик по клетке рядом с
//!   заводом — `CURSOR_ACTIVATE`, приказ `ORDER_ACTIVATE` получает **завод**;
//! * `UnitsManager_ActivateUnit` (`units_manager.cpp:3826-3913`): самолёт
//!   получает `ORDER_MOVE`/`INIT` и `ScaleUnit(EXPAND)`, затем взлетает
//!   (`ExecuteMove` → `ORDER_TAKE_OFF`), завод отдаёт `--storage` и запускает
//!   следующий заказ.

use max_core::enums::{unit_flags, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::GameState;

use max_app::hud;
use max_app::{App, AppConfig};

fn state() -> GameState {
    let mut state = GameState::new();
    state.teams[0].team_type = TeamKind::Player;
    state.teams[1].team_type = TeamKind::Computer;
    state.team_units[0] = TeamUnits::with_clan(max_core::TeamClan::Random);
    state.team_units[1] = TeamUnits::with_clan(max_core::TeamClan::Random);
    state.active_turn_team = TeamId::RED;
    state.player_team = TeamId::RED;
    state
}

/// AIRPLT в `(10,10)` с готовым истребителем «внутри» — состояние после
/// `UnitInfo::SpawnNewUnit`.
fn plant_with_fighter(state: &mut GameState) -> (max_core::UnitId, max_core::UnitId) {
    let plant = state
        .spawn_unit(UnitType::AirUnitsPlant, TeamId::RED, Point::new(10, 10), 0)
        .expect("air plant");
    let fighter = state
        .spawn_unit(UnitType::Fighter, TeamId::RED, Point::new(10, 10), 0)
        .expect("fighter");

    {
        let mut unit = state.unit(fighter).expect("fighter").clone();
        unit.parent_unit = Some(plant);
        unit.orders = UnitOrder::Idle;
        unit.state = UnitOrderState::Store;
        unit.scaler_adjust = 4;
        state.write_unit(fighter, unit);
    }
    {
        let mut unit = state.unit(plant).expect("plant").clone();
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
    let mut state = state();
    let (plant, fighter) = plant_with_fighter(&mut state);

    // Клетка рядом с заводом (2×2 в (10,10)-(11,11)) — `9,10`.
    let command = hud::build_activate_order(&state, fighter, Point::new(9, 10))
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
    let mut state = state();
    let (_plant, fighter) = plant_with_fighter(&mut state);

    // Внутри 2×2 завода C++ отдаёт `CURSOR_FRIEND` — приказа нет.
    assert!(hud::build_activate_order(&state, fighter, Point::new(10, 10)).is_none());
    assert!(hud::build_activate_order(&state, fighter, Point::new(11, 11)).is_none());

    // Слишком далеко — не «рядом с контейнером».
    assert!(hud::build_activate_order(&state, fighter, Point::new(20, 20)).is_none());
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

    assert!(hud::build_activate_order(&state, tank, Point::new(6, 5)).is_none());
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
        hud::build_air_move_order(&state, fighter, Point::new(13, 14)).expect("air move");
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

    // Наземному юниту воздушный приказ не строится.
    let tank = state
        .spawn_unit(
            UnitType::Tank,
            TeamId::RED,
            Point::new(5, 5),
            unit_flags::MOBILE_LAND_UNIT,
        )
        .expect("tank");
    assert!(hud::build_air_move_order(&state, tank, Point::new(6, 5)).is_none());
}

#[test]
fn app_launches_the_fighter_out_of_the_air_plant() {
    let mut app = App::new(AppConfig::default());
    *app.state_mut() = state();
    let (_plant, fighter) = plant_with_fighter(app.state_mut());

    // Игрок: выделен готовый юнит (после `PlaceNewUnit`), клик рядом с заводом.
    app.hud_mut().select(Some(fighter));
    let command = hud::build_activate_order(app.state(), fighter, Point::new(9, 10))
        .expect("приказ вывода");
    assert_eq!(
        app.apply(command),
        max_core::ApplyOutcome::Applied,
        "08/P3 применён"
    );

    for _ in 0..64 {
        app.tick();

        let Some(unit) = app.state().unit(fighter) else {
            break;
        };
        if unit.flags & unit_flags::HOVERING != 0 && unit.grid != Point::new(10, 10) {
            break;
        }
    }

    let unit = app.state().unit(fighter).expect("fighter alive");
    assert_ne!(unit.grid, Point::new(10, 10), "самолёт вылетел из завода");
    assert!(
        unit.flags & unit_flags::HOVERING != 0,
        "самолёт в воздухе (`ORDER_TAKE_OFF`)"
    );
    assert!(unit.parent_unit.is_none(), "самолёт больше не в заводе");
}

#[test]
fn unit_menu_on_a_ready_plant_selects_the_stored_fighter() {
    let mut app = App::new(AppConfig::default());
    *app.state_mut() = state();
    let (plant, fighter) = plant_with_fighter(app.state_mut());

    // Первый клик по заводу — выделение.
    app.hud_mut().select(Some(plant));
    // Повторный клик (меню команд) — `Popup_InitFactories` → `PlaceNewUnit`.
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
