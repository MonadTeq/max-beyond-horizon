//! Unit menu items B1, sounds D1 and pause D3 — integration checks.
//!
//! Oracle — `max-cpp-port`: `units_manager.cpp` (`Popup_OnClick_*`),
//! `game_manager.cpp` (`ProcessMouse`, `SetUnitOrder`, `StealUnit`,
//! `DisableUnit`, `PlayUnitStatusVoice`), `repairshopmenu.cpp` (activate),
//! `menu.cpp`/`dialogmenu.cpp` (pause).
//!
//! All checks go through the public `max_app::App`/`max_core` API, i.e.
//! the same path as player input.

use max_app::sfx::{self, Sfx};
use max_app::ui::{handle_session_input, InputEvent, Key, Transition};
use max_app::{App, AppConfig, LocalSetup};
use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::universe::StateRef;
use max_core::team::TeamId;
use max_core::unit::UnitId;

fn local_app() -> (App, TeamId) {
    let mut app = App::new(AppConfig::default());
    let setup = LocalSetup::default();

    if app.start_local_game(&setup).is_err() {
        // Without map data a local match cannot be set up — the test is skipped,
        // like the other integration tests of the repository.
        return (app, TeamId::RED);
    }

    let team = app.player_team();

    (app, team)
}

/// A free cell near the map center.
fn free_point(app: &App, near: Point) -> Point {
    for radius in 0..12 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let point = Point::new(near.x + dx, near.y + dy);

                if app.state_ref().map().index(point.x, point.y).is_some()
                    && app.state_ref().units_at(point).is_empty()
                {
                    return point;
                }
            }
        }
    }

    near
}

fn spawn(app: &mut App, unit_type: UnitType, team: TeamId, near: Point) -> Option<UnitId> {
    let flags = max_core::access::unit_type_flags(unit_type);
    let point = free_point(app, near);

    app.universe_mut().spawn_unit(unit_type, team, point, flags)
}

/// `attack` — the `targeting_mode` toggle (`UnitsManager_Popup_OnClick_TargetingMode`).
#[test]
fn attack_item_toggles_targeting_mode() {
    let (mut app, team) = local_app();

    if !app.local_active() {
        return;
    }

    let center = Point::new(
        app.state_ref().map().size.width / 2,
        app.state_ref().map().size.height / 2,
    );
    let Some(tank) = spawn(&mut app, UnitType::Tank, team, center) else {
        return;
    };

    app.hud_mut().select(Some(tank));
    assert!(app.open_unit_menu(), "меню танка открывается");

    let index = app
        .unit_menu()
        .and_then(|menu| menu.index_of(max_app::unitmenu::ItemId::Attack))
        .expect("пункт attack");

    assert!(!app.hud().targeting_mode);
    app.activate_unit_menu_item(index);
    assert!(app.hud().targeting_mode, "attack включает targeting_mode");

    app.open_unit_menu();
    app.activate_unit_menu_item(index);
    assert!(!app.hud().targeting_mode, "повторный клик выключает режим");
}

/// `load` — cursor mode 7 (`UnitsManager_Popup_OnClick_Load`), and a click on
/// the passenger gives `ORDER_MOVE_TO_UNIT` into the transport cell
/// (`GameManager_ProcessMouse`, CURSOR_LOAD).
#[test]
fn load_mode_orders_move_to_unit_and_lands_on_arrival() {
    let (mut app, team) = local_app();

    if !app.local_active() {
        return;
    }

    let center = Point::new(
        app.state_ref().map().size.width / 2,
        app.state_ref().map().size.height / 2,
    );
    let Some(carrier) = spawn(&mut app, UnitType::ArmouredPersonnelCarrier, team, center) else {
        return;
    };
    let Some(infantry) = spawn(
        &mut app,
        UnitType::Infantry,
        team,
        Point::new(center.x + 4, center.y),
    ) else {
        return;
    };

    app.hud_mut().select(Some(carrier));
    assert!(app.open_unit_menu());

    let index = app
        .unit_menu()
        .and_then(|menu| menu.index_of(max_app::unitmenu::ItemId::Load))
        .expect("пункт load");

    app.activate_unit_menu_item(index);
    assert_eq!(
        app.hud().cursor_mode,
        max_app::unitmenu::CURSOR_LOAD,
        "load включает курсорный режим 7"
    );

    // A click on an own infantryman: **it** gets the order (C++ `GameManager_Unit`).
    let state = app.state_copy();
    let command = max_app::unitmenu::load_mode_click(StateRef::from(&state), carrier, infantry)
        .expect("load click")
        .expect("приказ");

    match &command {
        max_core::input::Command::Order(packet) => {
            assert_eq!(packet.unit, infantry, "двигается пассажир");
            match &packet.payload {
                max_core::input::OrderPayload::P3(p3) => {
                    assert_eq!(p3.p2.p1.order, UnitOrder::MoveToUnit);
                    assert_eq!(p3.move_to, state.unit(carrier).expect("транспорт").grid);
                }
                other => panic!("ожидался P3, получен {other:?}"),
            }
        }
        other => panic!("ожидался Order, получен {other:?}"),
    }

    // The second click, when the passenger is already in the transport cell and ready:
    // `ORDER_LAND`/`INIT` with `parent = transport` (`game_manager.cpp:6824`).
    let mut same_cell = app.state_copy();
    let carrier_grid = same_cell.unit(carrier).expect("транспорт").grid;
    if let Some(unit) = same_cell.unit_mut(infantry) {
        unit.grid = carrier_grid;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.orders = UnitOrder::Await;
    }
    let command = max_app::unitmenu::load_mode_click(StateRef::from(&same_cell), carrier, infantry)
        .expect("load same cell")
        .expect("приказ");

    match &command {
        max_core::input::Command::Order(packet) => match &packet.payload {
            max_core::input::OrderPayload::P2(p2) => {
                assert_eq!(p2.p1.order, UnitOrder::Land);
                assert_eq!(p2.p1.state, UnitOrderState::Init);
                assert_eq!(
                    p2.parent_unit,
                    Some(carrier),
                    "пассажир садится в транспорт"
                );
            }
            other => panic!("ожидался P2, получен {other:?}"),
        },
        other => panic!("ожидался Order, получен {other:?}"),
    }
}

/// `steal`/`disable` — cursor modes 8/9: the `ORDER_AWAIT_STEAL_UNIT`/
/// `AWAIT_DISABLE_UNIT`/INIT with the target as parent and a roll
/// (`GameManager_StealUnit`/`DisableUnit`).
#[test]
fn infiltrator_orders_steal_and_disable_with_parent() {
    let (app, team) = local_app();

    if !app.local_active() {
        return;
    }

    let mut state = app.state_copy();
    let center = Point::new(state.map.size.width / 2, state.map.size.height / 2);
    let point = free_point(&app, center);
    let commando = state
        .spawn_unit(
            UnitType::Infiltrator,
            team,
            point,
            max_core::access::unit_type_flags(UnitType::Infiltrator),
        )
        .expect("диверсант");
    let enemy_team = TeamId(1);
    let target_point = Point::new(point.x + 1, point.y);
    let target = state
        .spawn_unit(
            UnitType::Tank,
            enemy_team,
            target_point,
            max_core::access::unit_type_flags(UnitType::Tank),
        )
        .expect("цель");

    // The target must be valid: an electronic unit, not in `IDLE`, visible.
    if let Some(unit) = state.unit_mut(target) {
        unit.set_visible_to_team(team, true);
        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;
    }
    if let Some(unit) = state.unit_mut(commando) {
        unit.shots = 1;
    }

    for (cursor, order) in [
        (max_app::unitmenu::CURSOR_STEAL, UnitOrder::AwaitStealUnit),
        (
            max_app::unitmenu::CURSOR_DISABLE,
            UnitOrder::AwaitDisableUnit,
        ),
    ] {
        let command = max_app::unitmenu::stealth_mode_click(StateRef::from(&state), commando, target, cursor, 42)
            .expect("stealth click")
            .expect("приказ");

        match &command {
            max_core::input::Command::Order(packet) => {
                assert_eq!(packet.unit, commando);
                match &packet.payload {
                    max_core::input::OrderPayload::P3(p3) => {
                        assert_eq!(p3.p2.p1.order, order);
                        assert_eq!(p3.p2.p1.state, UnitOrderState::Init);
                        assert_eq!(p3.p2.parent_unit, Some(target), "parent = цель");
                        assert_eq!(p3.stealth_dice_roll, 42);
                    }
                    other => panic!("ожидался P3, получен {other:?}"),
                }
            }
            other => panic!("ожидался Order, получен {other:?}"),
        }
    }

    // Without shots — the C++ message (`_(f37a)`/`_(3e56)`).
    let mut empty = state.clone();
    if let Some(unit) = empty.unit_mut(commando) {
        unit.shots = 0;
    }
    let error = max_app::unitmenu::stealth_mode_click(
        StateRef::from(&empty),
        commando,
        target,
        max_app::unitmenu::CURSOR_STEAL,
        42,
    )
    .expect_err("нет действия");
    assert!(error.contains("Infiltrator"));
}

/// Bulldozer: `Access_GetRemovableRubble` + `ORDER_CLEAR` with `build_time` 4/1
/// (`UnitsManager_Popup_OnClick_StopRemove`).
#[test]
fn bulldozer_remove_orders_clear_with_parent_and_build_time() {
    let (mut app, team) = local_app();

    if !app.local_active() {
        return;
    }

    let center = Point::new(
        app.state_ref().map().size.width / 2,
        app.state_ref().map().size.height / 2,
    );
    let Some(_bulldozer) = spawn(&mut app, UnitType::Bulldozer, team, center) else {
        return;
    };
    // Small rubble — in the bulldozer cell (C++ takes it from `Hash_MapHash`).
    let bulldozer = app
        .hud()
        .selected
        .or_else(|| {
            app.state_ref().unit_ids_in_order().into_iter().find(|id| {
                app.state_ref()
                    .unit(*id)
                    .is_some_and(|unit| unit.unit_type == UnitType::Bulldozer)
            })
        })
        .expect("бульдозер");
    let grid = app.state_ref().unit(bulldozer).expect("бульдозер").grid;
    let rubble = app
        .universe_mut()
        .spawn_unit(
            UnitType::SmallRubble,
            team,
            grid,
            max_core::access::unit_type_flags(UnitType::SmallRubble),
        )
        .expect("обломки");

    assert_eq!(
        max_app::unitmenu::removable_rubble(app.state_ref(), team, grid),
        Some(rubble)
    );

    app.hud_mut().select(Some(bulldozer));
    assert!(app.open_unit_menu());
    let index = app
        .unit_menu()
        .and_then(|menu| menu.index_of(max_app::unitmenu::ItemId::RemoveRubble))
        .expect("пункт remove");
    app.activate_unit_menu_item(index);

    assert_eq!(
        app.state_ref().unit(bulldozer).expect("бульдозер").build_time,
        1
    );
    assert_eq!(
        app.state_ref().unit(bulldozer).expect("бульдозер").parent_unit,
        Some(rubble)
    );

    // `Popup_OnClick_StopRemove` — `ORDER_CLEAR`/`INIT` with a parent goes into
    // the order queue (the UI path: applied by `flush_orders`/the network).
    let has_clear = app.hud().pending.iter().any(|command| {
        matches!(command, max_core::input::Command::Order(packet)
            if matches!(&packet.payload, max_core::input::OrderPayload::P3(p3)
                if p3.p2.p1.order == UnitOrder::Clear
                    && p3.p2.p1.state == UnitOrderState::Init
                    && p3.p2.parent_unit == Some(rubble)))
    });
    assert!(has_clear, "ORDER_CLEAR в очереди приказов");
}

/// `Upg. All` — `ORDER_UPGRADE`/`EXECUTING_ORDER` and debiting materials
/// (`UnitsManager_Popup_OnClick_UpgradeAll`).
#[test]
fn upgrade_all_spends_complex_materials_and_sends_orders() {
    let (mut app, team) = local_app();

    if !app.local_active() {
        return;
    }

    let center = Point::new(
        app.state_ref().map().size.width / 2,
        app.state_ref().map().size.height / 2,
    );
    let Some(station) = spawn(&mut app, UnitType::PowerStation, team, center) else {
        return;
    };

    // The upgrade is available: the current values are above the base.
    if let Some(team_units) = app.universe_mut().team_units_table_mut().get_mut(team.0 as usize) {
        let values = team_units.current_unit_values_mut(UnitType::PowerStation as u16);
        let hits = values.get_attribute(max_core::unit_values::ATTRIB_HITS);
        values.set_attribute(max_core::unit_values::ATTRIB_HITS, hits + 10);
    }
    // The complex has materials (C++ `GetCargoInfo`).
    if let Some(complex) = app.state_ref().unit(station).and_then(|unit| unit.complex) {
        if let Some(complex) = app.universe_mut().complex_mut(team, complex.0 as u16) {
            complex.raw = 120;
        }
    }

    let report = max_app::unitmenu::upgrade_all_orders(app.state_ref(), station).expect("отчёт");
    assert_eq!(report.count, 1, "{}", report.message);
    assert_eq!(report.commands.len(), 1);
    assert!(report.material_cost > 0);

    match &report.commands[0] {
        max_core::input::Command::Order(packet) => match &packet.payload {
            max_core::input::OrderPayload::P2(p2) => {
                assert_eq!(p2.p1.order, UnitOrder::Upgrade);
                assert_eq!(p2.p1.state, UnitOrderState::ExecutingOrder);
                assert_eq!(p2.parent_unit, Some(station), "SetParent(self)");
            }
            other => panic!("ожидался P2, получен {other:?}"),
        },
        other => panic!("ожидался Order, получен {other:?}"),
    }
}

/// `remove` — the `SelfDestructMenu` dialog: `Arm`, `SLFDOPN` frames, `OK` →
/// `ORDER_EXPLODE`/`EXPLODE`.
#[test]
fn self_destruct_dialog_arms_and_orders_explode() {
    let (mut app, team) = local_app();

    if !app.local_active() {
        return;
    }

    let center = Point::new(
        app.state_ref().map().size.width / 2,
        app.state_ref().map().size.height / 2,
    );
    let Some(station) = spawn(&mut app, UnitType::PowerStation, team, center) else {
        return;
    };

    app.hud_mut().select(Some(station));
    assert!(app.open_unit_menu());
    let index = app
        .unit_menu()
        .and_then(|menu| menu.index_of(max_app::unitmenu::ItemId::RemoveStationary))
        .expect("пункт remove");
    app.activate_unit_menu_item(index);

    assert!(app.self_destruct().is_some(), "диалог открылся");

    // `Arm` → animation; without advancing the clock `OK` does not confirm.
    app.self_destruct_input(InputEvent::Key(Key::Enter));
    assert_eq!(
        app.self_destruct().map(|menu| menu.stage),
        Some(max_app::selfdestruct::Stage::Armed)
    );
    let ok = app.self_destruct().expect("диалог").ok_rect();
    let action = app.self_destruct_input(InputEvent::MouseClick(ok.ulx + 2, ok.uly + 2));
    assert_eq!(
        action,
        Some(max_app::selfdestruct::SelfDestructAction::None)
    );

    // `advance` plays 6 frames of 48 ms.
    app.advance_self_destruct(0.5);
    assert!(app.self_destruct().is_some_and(|menu| menu.is_armed()));

    let ok = app.self_destruct().expect("диалог").ok_rect();
    let action = app.self_destruct_input(InputEvent::MouseClick(ok.ulx + 2, ok.uly + 2));
    assert_eq!(
        action,
        Some(max_app::selfdestruct::SelfDestructAction::Explode)
    );
    assert!(app.self_destruct().is_none());

    // The `ORDER_EXPLODE` order got into the queue.
    let has_explode = app.hud().pending.iter().any(|command| {
        matches!(command, max_core::input::Command::Order(packet)
            if matches!(&packet.payload, max_core::input::OrderPayload::P1(p1)
                if p1.order == UnitOrder::Explode && p1.state == UnitOrderState::Explode))
    });
    assert!(has_explode, "ORDER_EXPLODE в очереди приказов");
}

/// `Alt+P` — `PauseMenu_Menu`: the "Game Paused" dialog and the tick pause.
#[test]
fn alt_p_opens_pause_dialog_and_resumes() {
    let (mut app, _team) = local_app();

    if !app.local_active() {
        return;
    }

    assert!(matches!(
        handle_session_input(&mut app, InputEvent::Key(Key::Alt('P')), false),
        Transition::None
    ));
    assert!(app.pause_dialog().is_some(), "Alt+P открывает диалог");
    assert!(
        app.local().map(|local| local.paused).unwrap_or(false),
        "тики локальной партии стоят"
    );

    // A click on `OK` closes it and returns the game.
    let ok = app.pause_dialog().expect("диалог").ok_rect();
    handle_session_input(
        &mut app,
        InputEvent::MouseClick(ok.ulx + 2, ok.uly + 2),
        false,
    );
    assert!(app.pause_dialog().is_none());
    assert!(!app.local().map(|local| local.paused).unwrap_or(true));
}

/// D1: a click on the map/panel/popup gives C++ sounds; `disable_fx` and
/// `disable_voice` are taken into account.
#[test]
fn ui_events_queue_cpp_sounds() {
    let (mut app, team) = local_app();

    if !app.local_active() {
        return;
    }

    let center = Point::new(
        app.state_ref().map().size.width / 2,
        app.state_ref().map().size.height / 2,
    );
    let _ = spawn(&mut app, UnitType::Tank, team, center);

    app.take_sounds();
    // A click on the map — `KCARG0` (`game_manager.cpp:6711`).
    let _ = handle_session_input(&mut app, InputEvent::MouseClick(300, 200), false);
    assert_eq!(app.take_sounds(), vec![Sfx::Click]);

    // A panel button — `MBUTT0`.
    let zoom = max_render::layout::ZOOM_PLUS;
    let _ = handle_session_input(
        &mut app,
        InputEvent::MouseClick(zoom.ulx + 2, zoom.uly + 2),
        false,
    );
    assert_eq!(app.take_sounds(), vec![Sfx::Button]);

    // Pause: `Alt+P` (MBUTT0) and `Esc` (NCANC0).
    let _ = handle_session_input(&mut app, InputEvent::Key(Key::Alt('p')), false);
    assert_eq!(app.take_sounds(), vec![Sfx::Button]);
    let _ = handle_session_input(&mut app, InputEvent::Key(Key::Escape), false);
    assert_eq!(app.take_sounds(), vec![Sfx::Cancel]);

    // Voices are disabled by `disable_voice`, effects — by `disable_fx`.
    let mut prefs = app.preferences().clone();
    prefs.disable_voice = true;
    app.set_preferences(prefs);
    assert!(!app.preferences().voice_enabled());
    assert!(app.preferences().sound_enabled());

    let mut prefs = app.preferences().clone();
    prefs.disable_fx = true;
    app.set_preferences(prefs);
    assert!(!app.sound_enabled());
}

/// The unit status voice — `GameManager_PlayUnitStatusVoice`, including `V_M093`
/// for a powered-on Research Center.
#[test]
fn unit_status_voice_matches_cpp_chain() {
    let mut unit = max_core::unit::UnitInfo::new(UnitType::ResearchCentre, TeamId::RED, UnitId(1));
    unit.orders = UnitOrder::PowerOn;
    unit.state = UnitOrderState::ExecutingOrder;
    unit.speed = 1;
    unit.ammo = 1;

    let (start, status) = sfx::unit_status_voice(&unit);
    assert_eq!(start, "V_START");
    assert_eq!(status, Some("V_M093"));
}
