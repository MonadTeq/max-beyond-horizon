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
    grid: max_core::geo::Point,
) -> UnitId {
    let flags = max_core::access::unit_type_flags(unit_type);
    state
        .spawn_unit(unit_type, TeamId::RED, grid, flags)
        .expect("юнит")
}

/// `UnitInfo::ProcessLoading` hold state: `ORDER_IDLE`/`STORE`, parent = carrier.
fn store(
    state: &mut max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
    carrier: UnitId,
    cargo: UnitId,
) {
    let unit = state.unit_mut(cargo).expect("пассажир");
    unit.orders = UnitOrder::Idle;
    unit.state = max_core::enums::UnitOrderState::Store;
    unit.parent_unit = Some(carrier);
}

fn carrier_with_units(
    unit_type: UnitType,
    cargo_type: UnitType,
    count: usize,
) -> (
    max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
    UnitId,
    Vec<UnitId>,
) {
    let mut state = test_state();
    let carrier = spawn(&mut state, unit_type, max_core::geo::Point::new(22, 22));
    let mut cargo = Vec::new();

    for index in 0..count {
        let id = spawn(
            &mut state,
            cargo_type,
            max_core::geo::Point::new(30 + index as i16, 30),
        );
        store(&mut state, carrier, id);
        cargo.push(id);
    }

    (state, carrier, cargo)
}

/// `RepairShopMenu::RepairShopMenu` (`repairshopmenu.cpp:384-424`): the six
/// generic slots in a 3×2 grid and the four `HANGAR` slots in a 2×2 grid.
#[test]
fn slot_rects_match_the_oracle_layout() {
    let (state, carrier, _) =
        carrier_with_units(UnitType::ArmouredPersonnelCarrier, UnitType::Infantry, 1);
    let menu = RepairShopMenu::new(StateRef::from(&state), carrier).expect("меню");
    assert_eq!(menu.slots_per_screen, SLOTS_PER_SCREEN);
    assert!(!menu.is_hangar);

    // `slot_window_ulx = (index % 3) * 155 + 17`, `uly = (index / 3) * 235 + 9`.
    assert_eq!(menu.slot_image_rect(0), PixelRect::new(17, 9, 128, 128));
    assert_eq!(menu.slot_image_rect(1), PixelRect::new(172, 9, 128, 128));
    assert_eq!(menu.slot_image_rect(2), PixelRect::new(327, 9, 128, 128));
    assert_eq!(menu.slot_image_rect(3), PixelRect::new(17, 244, 128, 128));
    assert_eq!(menu.slot_image_rect(5), PixelRect::new(327, 244, 128, 128));
    // `unit_stats_bounds = (ulx + 3, uly + 140)-(ulx + 125, uly + 170)`.
    assert_eq!(menu.slot_stats_rect(0), PixelRect::new(20, 149, 122, 30));
    // `button_activate` at `stats.ulx - 12`, `stats.uly + 42`.
    assert_eq!(menu.activate_rect(0), PixelRect::new(8, 191, 70, 21));

    // `HANGAR`: `ulx = (index & 1) * 226 + 18`, `uly = (index / 2) * 235 + 9`,
    // picture 200×128, stats at `ulx + 38`.
    let (state, hangar, _) = carrier_with_units(UnitType::Hangar, UnitType::Fighter, 1);
    let menu = RepairShopMenu::new(StateRef::from(&state), hangar).expect("меню ангара");
    assert_eq!(menu.slots_per_screen, HANGAR_SLOTS_PER_SCREEN);
    assert_eq!(menu.slot_image_rect(0), PixelRect::new(18, 9, 200, 128));
    assert_eq!(menu.slot_image_rect(1), PixelRect::new(244, 9, 200, 128));
    assert_eq!(menu.slot_image_rect(2), PixelRect::new(18, 244, 200, 128));
    assert_eq!(menu.slot_stats_rect(0), PixelRect::new(56, 149, 122, 30));
    assert_eq!(menu.activate_rect(0), PixelRect::new(44, 191, 70, 21));
}

/// `RepairShopMenu_OnClick_UpArrow`/`DownArrow` (`:918-942`) page by
/// `unit_slots_per_screen`; `UpdateButtons` (`:1008-1025`) disables the arrows
/// at the ends.
#[test]
fn arrows_page_by_slots_per_screen() {
    let (state, carrier, cargo) =
        carrier_with_units(UnitType::ArmouredPersonnelCarrier, UnitType::Infantry, 7);
    let mut menu = RepairShopMenu::new(StateRef::from(&state), carrier).expect("меню");

    // `Access_IsHeldByUnit` walks the mobile list from `Begin()`; the list is
    // `PushFront`-ed (`UnitInfo::AddToDrawList`), so slot 0 is the unit loaded
    // **last** and the oldest stored unit is at the end of the list.
    let order: Vec<UnitId> = cargo.iter().rev().copied().collect();

    assert!(!menu.can_page_up(), "первая страница — up выключен");
    assert!(menu.can_page_down());
    assert_eq!(menu.visible_unit(0), Some(order[0]));

    menu.page_down();
    assert_eq!(menu.slot_index, SLOTS_PER_SCREEN);
    assert!(menu.can_page_up());
    assert!(!menu.can_page_down(), "7 units — вторая страница последняя");
    assert_eq!(menu.visible_unit(0), Some(order[6]));
    assert_eq!(menu.visible_unit(1), None);

    // A click on the down arrow on the last page changes nothing (`:937`).
    let before = menu.slot_index;
    assert_eq!(
        menu.click(PAGE_DOWN_RECT.ulx + 1, PAGE_DOWN_RECT.uly + 1),
        RepairShopAction::None
    );
    assert_eq!(menu.slot_index, before);

    menu.page_up();
    assert_eq!(menu.slot_index, 0);
    menu.page_up();
    assert_eq!(menu.slot_index, 0, "up обрезается нулём (`:925`)");
}

/// `RepairShopSlot_OnClick_Activate` (`:737-745`): the button of a slot releases
/// **that** stored unit, not the first one.
#[test]
fn activate_button_targets_the_slot_unit() {
    let (state, carrier, cargo) =
        carrier_with_units(UnitType::ArmouredPersonnelCarrier, UnitType::Infantry, 3);
    let mut menu = RepairShopMenu::new(StateRef::from(&state), carrier).expect("меню");
    let order: Vec<UnitId> = cargo.iter().rev().copied().collect();

    for slot in 0..3 {
        let rect = menu.activate_rect(slot);
        assert_eq!(
            menu.click(rect.ulx + 1, rect.uly + 1),
            RepairShopAction::Activate(order[slot]),
            "слот {slot}"
        );
    }

    // The `1..6` hotkeys are the headless extension of the same action.
    assert_eq!(
        menu.key(Key::Char('2')),
        RepairShopAction::Activate(order[1])
    );
    assert_eq!(menu.key(Key::Escape), RepairShopAction::Done);
    assert_eq!(menu.key(Key::Char('?')), RepairShopAction::Help);
    assert_eq!(
        menu.click(DONE_RECT.ulx + 1, DONE_RECT.uly + 1),
        RepairShopAction::Done
    );
    assert_eq!(
        menu.click(HELP_RECT.ulx + 1, HELP_RECT.uly + 1),
        RepairShopAction::Help
    );

    // A click on the picture/stats area is no action (the oracle has only the
    // button handler).
    assert_eq!(menu.click(50, 50), RepairShopAction::None);
}

/// `RepairShopMenu::RepairShopMenu` is only reachable for the repair-shop types
/// (`UnitsManager_Popup_InitRepairShops`).
#[test]
fn only_repair_shop_types_open_the_screen() {
    let (state, transport, _) = carrier_with_units(UnitType::SeaTransport, UnitType::Tank, 1);
    assert!(RepairShopMenu::new(StateRef::from(&state), transport).is_some());

    for unit_type in [
        UnitType::Barracks,
        UnitType::Depot,
        UnitType::Hangar,
        UnitType::Dock,
    ] {
        assert!(is_repair_shop(unit_type), "{unit_type:?}");
    }

    let mut state = test_state();
    let tank = spawn(
        &mut state,
        UnitType::Tank,
        max_core::geo::Point::new(22, 22),
    );
    assert!(RepairShopMenu::new(StateRef::from(&state), tank).is_none());
    assert!(RepairShopMenu::new(StateRef::from(&state), UnitId(9999)).is_none());
}
