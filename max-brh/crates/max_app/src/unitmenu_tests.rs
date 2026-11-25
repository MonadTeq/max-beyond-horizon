use super::*;
use max_core::enums::TeamClan;
use max_core::team::TeamId;

fn test_state() -> max_core::universe::GameUniverse<max_core::universe::LocalDispatcher> {
    let mut state = max_core::universe::GameUniverse::default();
    *state.team_units_mut(TeamId::RED).expect("RED economy") =
        max_core::team::TeamUnits::with_clan(TeamClan::Random);
    state
}

fn spawn(
    state: &mut max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
    unit_type: UnitType,
) -> UnitId {
    let flags = max_core::access::unit_type_flags(unit_type);
    let id = state
        .spawn_unit(unit_type, max_core::team::TeamId::RED, Point::new(22, 22), flags)
        .expect("юнит");
    id
}

#[test]
fn tank_menu_matches_cpp_order_and_contents() {
    let mut state = test_state();
    let id = spawn(&mut state, UnitType::Tank);
    let unit = state.snapshot().unit(id).expect("танк").clone();
    let items = items_for(StateRef::from(&state), &unit);
    let names: Vec<&str> = items.iter().map(|item| item.id.name()).collect();

    // `InitMilitary`: attack('3') → commons x-fer? (the tank has no cargo) →
    // enter('5') → sentry('8') → Done('9'). Sorting by position.
    assert_eq!(names, vec!["attack", "enter", "sentry", "Done"]);

    // `attack` and `sentry` — positions 3 and 8, `Done` — 9.
    assert_eq!(items[0].id, ItemId::Attack);
    assert!(
        items[0].enabled,
            "attack — тумблер targeting_mode (UnitsManager_Popup_OnClick_TargetingMode)"
    );
    assert_eq!(items[1].id, ItemId::Enter);
    assert!(items[1].enabled);
    assert_eq!(items[2].id, ItemId::Sentry);
    assert!(items[2].enabled, "sentry — приказ P1");
    assert_eq!(items[3].id, ItemId::Done);
}

#[test]
fn scout_menu_is_military_and_scanner_is_commons() {
    let mut state = test_state();

    // `SCOUT` → `UnitsManager_PopupCallbacks[3]` (`InitMilitary`): the manual-run
    // defect was that the Scout fell through to `InitCommons`, so a player could
    // not enter the targeting mode from its popup — the attack item was absent.
    let scout = spawn(&mut state, UnitType::Scout);
    let scout_unit = state.snapshot().unit(scout).expect("scout").clone();
    let items = items_for(StateRef::from(&state), &scout_unit);

    let attack = items
        .iter()
        .find(|item| item.id == ItemId::Attack)
        .expect("Scout must have the attack item (oracle InitMilitary)");
    assert!(attack.enabled, "attack must be a live targeting toggle");

    // `SCANNER` → callbacks `[1]` (`InitCommons`), not military: no attack item.
    let scanner = spawn(&mut state, UnitType::Scanner);
    let scanner_unit = state.snapshot().unit(scanner).expect("scanner").clone();
    let scanner_items = items_for(StateRef::from(&state), &scanner_unit);
    assert!(
        !scanner_items.iter().any(|item| item.id == ItemId::Attack),
        "Scanner is InitCommons in the oracle, not InitMilitary: {:?}",
        scanner_items
            .iter()
            .map(|item| item.id.name())
            .collect::<Vec<_>>()
    );
}

#[test]
fn constructor_menu_has_build_first() {
    let mut state = test_state();
    let id = spawn(&mut state, UnitType::Constructor);
    let unit = state.snapshot().unit(id).expect("строитель").clone();
    let items = items_for(StateRef::from(&state), &unit);

    assert_eq!(items[0].id, ItemId::Build);
    assert!(items[0].enabled, "Build открывает BuildMenu");
    // The builder — `RAW` cargo, so x-fer is in the list and enabled
    // (`TransferMenu_Menu` + `ProcessOrderTransfer` are ported).
    let transfer = items
        .iter()
        .find(|item| item.id == ItemId::Transfer)
        .expect("x-fer у строителя");
    assert!(transfer.enabled, "x-fer открывает TransferMenu");
    assert!(transfer.note.is_empty());
}

#[test]
fn stop_appears_only_with_path() {
    let mut state = test_state();
    let id = spawn(&mut state, UnitType::Tank);
    let mut unit = state.snapshot().unit(id).expect("танк").clone();
    assert!(!items_for(StateRef::from(&state), &unit)
        .iter()
        .any(|item| item.id == ItemId::Stop));

    unit.path = Some(max_core::unit::UnitPath::Ground(max_core::unit::GroundPath {
        end_x: 24,
        end_y: 22,
        step_index: 0,
        steps: Vec::new(),
    }));

    assert!(items_for(StateRef::from(&state), &unit)
        .iter()
        .any(|item| item.id == ItemId::Stop && item.enabled));
}

#[test]
fn builder_mid_build_offers_enabled_stop_instead_of_build() {
    let mut state = test_state();
    let id = spawn(&mut state, UnitType::Constructor);
    let mut unit = state.snapshot().unit(id).expect("строитель").clone();

    // C++ `UnitsManager_Popup_InitBuilders`: during `ORDER_BUILD` the item
    // `Build` is replaced with `stop` (`GameManager_SelectBuildSite`).
    unit.orders = UnitOrder::Build;
    unit.state = UnitOrderState::BuildInProgress;
    unit.build_list = vec![UnitType::TrainingHall];
    unit.build_time = 10;
    state.write_unit(id, unit.clone());

    let items = items_for(StateRef::from(&state), &unit);
    assert!(
        !items.iter().any(|item| item.id == ItemId::Build),
            "во время стройки `Build` заменяется на `stop` (как C++)"
    );
    let stop = items
        .iter()
        .find(|item| item.id == ItemId::StopBuild)
        .expect("stop во время стройки");
    assert!(
        stop.enabled,
            "stop отменяет стройку (GameManager_SelectBuildSite) — не серый"
    );
    assert!(stop.note.is_empty());
    assert_eq!(stop.hotkey, '7');
}

#[test]
fn halted_factory_offers_enabled_start() {
    let mut state = test_state();
    let id = spawn(&mut state, UnitType::TrainingHall);
    let mut unit = state.snapshot().unit(id).expect("завод").clone();
    unit.orders = UnitOrder::HaltBuilding2;
    unit.state = UnitOrderState::ExecutingOrder;
    unit.build_list = vec![UnitType::Infantry];
    unit.build_time = 2;
    state.write_unit(id, unit.clone());

    let start = items_for(StateRef::from(&state), &unit)
        .into_iter()
        .find(|item| item.id == ItemId::StartBuild)
        .expect("start у остановленного завода");
    assert!(
        start.enabled,
            "start продолжает стройку (UnitInfo::BuildOrder) — не серый"
    );
    assert!(start.note.is_empty());
}

#[test]
fn transport_lists_load_and_activate() {
    let mut state = test_state();
    let id = spawn(&mut state, UnitType::ArmouredPersonnelCarrier);
    let unit = state.snapshot().unit(id).expect("транспорт").clone();
    let items = items_for(StateRef::from(&state), &unit);

    let load = items
        .iter()
        .find(|item| item.id == ItemId::Load)
        .expect("load у транспорта");

    assert!(
        load.enabled,
            "load — тумблер курсорного режима 7 (UnitsManager_Popup_OnClick_Load)"
    );
    assert!(load.note.is_empty());

    // `activate` (RepairShopMenu) stays disabled — the screen is not ported.
    let activate = items
        .iter()
        .find(|item| item.id == ItemId::Activate)
        .expect("activate у транспорта");
    assert!(!activate.enabled);
    assert!(!activate.note.is_empty());
}

#[test]
fn infiltrator_has_stealth_and_targeting_items_enabled() {
    let mut state = test_state();
    let id = spawn(&mut state, UnitType::Infiltrator);
    let unit = state.snapshot().unit(id).expect("диверсант").clone();
    let items = items_for(StateRef::from(&state), &unit);

    for id in [ItemId::Disable, ItemId::Steal, ItemId::Attack] {
        let item = items
            .iter()
            .find(|item| item.id == id)
            .unwrap_or_else(|| panic!("{id:?} у диверсанта"));

        assert!(item.enabled, "{id:?} должен быть включён");
        assert!(item.note.is_empty(), "{id:?}: {}", item.note);
    }
}

#[test]
fn stationary_building_offers_enabled_remove_and_upgrade_all() {
    let mut state = test_state();
    let id = spawn(&mut state, UnitType::PowerStation);

    // The upgrade has something to improve: the team's current values are above the base.
    if let Some(team_units) = state.team_units_mut(TeamId::RED) {
        let values = team_units.current_unit_values_mut(UnitType::PowerStation as u16);
        let hits = values.get_attribute(ATTRIB_HITS);
        values.set_attribute(ATTRIB_HITS, hits + 10);
    }

    let unit = state.snapshot().unit(id).expect("здание").clone();
    let items = items_for(StateRef::from(&state), &unit);

    let remove = items
        .iter()
        .find(|item| item.id == ItemId::RemoveStationary)
        .expect("remove у здания");
    assert!(remove.enabled, "remove открывает UnitsManager_SelfDestructMenu");

    let upgrade_all = items
        .iter()
        .find(|item| item.id == ItemId::UpgradeAll)
        .expect("Upg. All у здания");
    assert!(upgrade_all.enabled, "Upg. All портирован");
}

#[test]
fn bulldozer_has_enabled_remove_rubble() {
    let mut state = test_state();
    let id = spawn(&mut state, UnitType::Bulldozer);
    let unit = state.snapshot().unit(id).expect("бульдозер").clone();
    let items = items_for(StateRef::from(&state), &unit);

    let remove = items
        .iter()
        .find(|item| item.id == ItemId::RemoveRubble)
        .expect("remove у бульдозера");
    assert!(remove.enabled, "бульдозер → Access_GetRemovableRubble");
    assert!(remove.note.is_empty());
}

#[test]
fn sentry_pressed_when_order_is_sentry() {
    let mut state = test_state();
    let id = spawn(&mut state, UnitType::Tank);
    let mut unit = state.snapshot().unit(id).expect("танк").clone();

    assert!(!items_for(StateRef::from(&state), &unit)
        .iter()
        .find(|item| item.id == ItemId::Sentry)
        .expect("sentry")
        .pressed);

    unit.orders = UnitOrder::Sentry;

    assert!(items_for(StateRef::from(&state), &unit)
        .iter()
        .find(|item| item.id == ItemId::Sentry)
        .expect("sentry")
        .pressed);
}

#[test]
fn layout_follows_cpp_geometry_and_stays_in_map() {
    let area = PixelRect::new(180, 18, 448, 448);
    let rect = UnitMenu::layout((400, 200), 4, area, 64);

    assert_eq!(rect.width, BUTTON_WIDTH + 6);
    assert_eq!(rect.height, BUTTON_HEIGHT * 4 + 5 * 3);
    assert_eq!(rect.ulx, 400);
    assert_eq!(rect.uly, 200 - rect.height / 2);

    // Right edge: shift left by width + one cell (`Gfx_ZoomLevel`).
    let rect = UnitMenu::layout((area.lrx() - 2, 100), 4, area, 64);
    assert_eq!(rect.ulx, area.lrx() - 2 - (BUTTON_WIDTH + 6) - 64);

    // The bottom edge is pinned to the map window.
    let rect = UnitMenu::layout((300, area.lry() - 2), 6, area, 64);
    assert_eq!(rect.lry(), area.lry());
}

#[test]
fn buttons_are_stacked_with_three_pixel_gap() {
    let mut state = test_state();
    let id = spawn(&mut state, UnitType::Tank);
    let unit = state.snapshot().unit(id).expect("танк").clone();
    let menu = UnitMenu::new(
        StateRef::from(&state),
        &unit,
        (400, 200),
        PixelRect::new(180, 18, 448, 448),
        64,
    )
    .expect("меню танка");

    assert_eq!(menu.button_rect(0).ulx, menu.rect.ulx + 3);
    assert_eq!(menu.button_rect(0).uly, menu.rect.uly + 3);

    for index in 1..menu.items.len() {
        assert_eq!(
            menu.button_rect(index).uly,
            menu.button_rect(index - 1).uly + BUTTON_HEIGHT + MARGIN
        );
    }
}

#[test]
fn hotkey_uses_position_then_first_letter() {
    let mut state = test_state();
    let id = spawn(&mut state, UnitType::Tank);
    let unit = state.snapshot().unit(id).expect("танк").clone();
    let menu = UnitMenu::new(
        StateRef::from(&state),
        &unit,
        (400, 200),
        PixelRect::new(180, 18, 448, 448),
        64,
    )
    .expect("меню");

    // '8' — the `sentry` position.
    assert_eq!(menu.key(Key::Char('8')), Some(MenuKey::Item(2)));
    // 'D' — the first letter of `Done`.
    assert_eq!(menu.key(Key::Char('d')), Some(MenuKey::Item(3)));
    assert_eq!(menu.key(Key::Escape), Some(MenuKey::Close));
    assert_eq!(menu.key(Key::Enter), None, "Enter в C++ попапе не работает");
    assert_eq!(menu.key(Key::Down), None, "стрелки в C++ попапе не работают");
}

#[test]
fn disabled_item_is_not_activated_by_click_or_key() {
    // Menu with a disabled item: in C++ a button without a handler does not react
    // (we only have `activate` of the repair shop left).
    let menu = UnitMenu {
        unit: UnitId(1),
        items: vec![
            UnitMenuItem::new(ItemId::Activate, "activate", '1', false)
                .disabled("RepairShopMenu не портирован"),
            UnitMenuItem::new(ItemId::Done, "Done", '9', false),
        ],
        rect: UnitMenu::layout((400, 200), 2, PixelRect::new(180, 18, 448, 448), 64),
        hover: None,
    };

    let rect = menu.button_rect(0);
    let centre = (rect.ulx + 1, rect.uly + 1);

    assert_eq!(
        menu_event(&menu, crate::ui::InputEvent::MouseClick(centre.0, centre.1)),
        MenuEvent::None,
            "отключённый пункт не срабатывает"
    );
    assert_eq!(
        menu_event(&menu, crate::ui::InputEvent::Key(Key::Char('1'))),
        MenuEvent::None
    );
    // An enabled item triggers.
    let done = menu.button_rect(1);
    assert_eq!(
        menu_event(
            &menu,
            crate::ui::InputEvent::MouseClick(done.ulx + 1, done.uly + 1)
        ),
        MenuEvent::Item(1)
    );
    // A click outside the menu — close.
    assert_eq!(
        menu_event(&menu, crate::ui::InputEvent::MouseClick(1, 1)),
        MenuEvent::Close
    );
}

#[test]
fn sentry_click_produces_p1_order() {
    let mut state = test_state();
    let id = spawn(&mut state, UnitType::Tank);
    let command = p1_order(StateRef::from(&state), id, UnitOrder::Sentry, UnitOrderState::ExecutingOrder)
        .expect("приказ");

    match command {
        max_core::input::Command::Order(packet) => {
            assert_eq!(packet.unit, id);
            match packet.payload {
                max_core::input::OrderPayload::P1(p1) => {
                    assert_eq!(p1.order, UnitOrder::Sentry);
                    assert_eq!(p1.state, UnitOrderState::ExecutingOrder);
                }
                other => panic!("ожидался P1, получен {other:?}"),
            }
        }
        other => panic!("ожидался Order, получен {other:?}"),
    }

    // Application: the order reaches the unit (`events::apply_command`).
    let mut state = test_state();
    let id = spawn(&mut state, UnitType::Tank);
    let command = p1_order(StateRef::from(&state), id, UnitOrder::Sentry, UnitOrderState::ExecutingOrder)
        .expect("приказ");
    state.apply_now(command.clone());
    assert_eq!(state.snapshot().unit(id).expect("танк").orders, UnitOrder::Sentry);
    assert_eq!(
        state.snapshot().unit(id).expect("танк").state,
        UnitOrderState::ExecutingOrder
    );
}

#[test]
fn repair_mode_click_needs_cargo_and_sets_parent() {
    let mut state = test_state();
    let repair = spawn(&mut state, UnitType::RepairUnit);
    let tank = spawn(&mut state, UnitType::Tank);

    state.unit_mut(repair).expect("repair").storage = 0;

    let error = cursor_mode_click(StateRef::from(&state), repair, tank, CURSOR_REPAIR).expect_err("нет груза");
    assert!(error.contains("storage"), "сообщение об отсутствии груза");

    state.unit_mut(repair).expect("repair").storage = 3;

    let command = cursor_mode_click(StateRef::from(&state), repair, tank, CURSOR_REPAIR)
        .expect("приказ")
        .expect("не пусто");

    state.apply_now(command.clone());
    let unit = state.snapshot().unit(repair).expect("repair");
    assert_eq!(unit.orders, UnitOrder::Repair);
    assert_eq!(unit.parent_unit, Some(tank));
}

#[test]
fn enter_mode_click_orders_move_to_unit() {
    let mut state = test_state();
    let tank = spawn(&mut state, UnitType::Tank);
    let transport = spawn(&mut state, UnitType::ArmouredPersonnelCarrier);

    state.unit_mut(transport).expect("транспорт").grid = Point::new(25, 22);
    state.unit_mut(tank).expect("танк").grid = Point::new(22, 22);

    let command = enter_mode_click(StateRef::from(&state), tank, transport).expect("приказ");

    match &command {
        max_core::input::Command::Order(packet) => {
            assert_eq!(packet.unit, tank);
        }
        other => panic!("ожидался Order, получен {other:?}"),
    }

    state.apply_now(command.clone());
    let unit = state.snapshot().unit(tank).expect("танк");
    assert_eq!(unit.orders, UnitOrder::MoveToUnit);
    assert_eq!(
        state.snapshot().order_data(tank).move_to,
        Point::new(25, 22),
            "цель — клетка транспорта"
    );
}

#[test]
fn factory_with_ready_unit_has_no_menu() {
    let mut state = test_state();
    let id = spawn(&mut state, UnitType::LightVehiclePlant);
    let mut unit = state.snapshot().unit(id).expect("завод").clone();
    unit.state = UnitOrderState::UnitReady;

    assert!(items_for(StateRef::from(&state), &unit).is_empty());
    assert!(UnitMenu::new(
        StateRef::from(&state),
        &unit,
        (400, 200),
        PixelRect::new(180, 18, 448, 448),
        64
    )
    .is_none());
}
