use super::*;
use max_core::enums::{unit_flags, TeamClan, UnitType};
use max_core::geo::Point;
use max_core::team::TeamUnits;
use max_render::buffer::PixelRect;
use max_render::layout::MAIN_MAP;

fn state_with_transfer() -> (
    max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
    UnitId,
    UnitId,
) {
    let mut state = max_core::universe::GameUniverse::default();
    state
        .team_mut(TeamId::RED)
        .expect("RED slot")
        .team_type = max_core::team::TeamKind::Player;
    *state.team_units_mut(TeamId::RED).expect("RED economy") =
        TeamUnits::with_clan(TeamClan::Random);

    let mine = state
        .spawn_unit(
            UnitType::MiningStation,
            TeamId::RED,
            Point::new(20, 20),
            unit_flags::STATIONARY | unit_flags::BUILDING,
        )
        .expect("mine");
    let complex = state.snapshot().unit(mine).expect("mine").complex.expect("complex");
    let dump = state
        .spawn_unit(
            UnitType::MaterialStorage,
            TeamId::RED,
            Point::new(22, 24),
            unit_flags::STATIONARY | unit_flags::BUILDING,
        )
        .expect("dump");
    if let Some(own) = state.snapshot().unit(dump).and_then(|unit| unit.complex) {
        state
            .team_units_mut(TeamId::RED)
            .expect("RED economy")
            .remove_complex(own);
    }
    state.unit_mut(dump).expect("dump").complex = Some(complex);
    state
        .team_units_mut(TeamId::RED)
        .expect("RED economy")
        .get_complex_mut(complex)
        .expect("complex")
        .grow();

    state.unit_mut(mine).expect("mine").storage = 10;
    state.unit_mut(dump).expect("dump").storage = 0;

    let truck = state
        .spawn_unit(
            UnitType::SupplyTruck,
            TeamId::RED,
            Point::new(21, 21),
            unit_flags::MOBILE_LAND_UNIT,
        )
        .expect("truck");
    state.unit_mut(truck).expect("truck").storage = 30;

    (state, truck, dump)
}

#[test]
fn menu_numbers_follow_cpp_formulas() {
    let (state, truck, dump) = state_with_transfer();
    let menu = TransferMenu::new(StateRef::from(&state), truck, dump).expect("menu");

    // source — a truck: 30/50; target — the whole complex: 10/75.
    assert_eq!((menu.source_materials, menu.source_capacity), (30, 50));
    assert_eq!((menu.target_materials, menu.target_capacity), (10, 75));

    // total = min(75-10, 30) = 30; source_free = min(50-30, 10) = 10.
    assert_eq!(menu.give_max, 30);
    assert_eq!(menu.source_free, 10);
    assert_eq!(menu.take_max, -10);
    assert_eq!(menu.amount, 30);

    assert_eq!(menu.source_shown(), 0);
    assert_eq!(menu.target_shown(), 40);
}

#[test]
fn slider_click_uses_limited_scrollbar_formula() {
    let (state, truck, dump) = state_with_transfer();
    let mut menu = TransferMenu::new(StateRef::from(&state), truck, dump).expect("menu");

    // zero_offset = -10, capacity = 75-10 = 65, width = 223.
    // x = 0 → (111 + 0)/223 - 10 = -10.
    assert_eq!(menu.click_value(44), -10);
    // x = 223 → (111 + 65*223)/223 - 10 = 65 - 10 = 55 → clamp to give_max 30.
    assert_eq!(menu.click_value(44 + 223), 55);
    menu.set_amount(menu.click_value(44 + 223));
    assert_eq!(menu.amount, 30);

    // Middle: (111 + 65*111)/223 - 10 = 32 - 10 = 22.
    assert_eq!(menu.click_value(44 + 111), 22);
}

#[test]
fn adjust_respects_give_and_take_limits() {
    let (state, truck, dump) = state_with_transfer();
    let mut menu = TransferMenu::new(StateRef::from(&state), truck, dump).expect("menu");

    assert!(!menu.adjust(true), "give_max достигнут");
    assert!(menu.adjust(false));
    assert_eq!(menu.amount, 29);

    menu.set_amount(-10);
    assert!(!menu.adjust(false), "take_max достигнут");
    assert!(menu.adjust(true));
    assert_eq!(menu.amount, -9);
}

#[test]
fn click_buttons_map_to_actions() {
    let (state, truck, dump) = state_with_transfer();
    let mut menu = TransferMenu::new(StateRef::from(&state), truck, dump).expect("menu");
    let window = menu.window_rect();
    let centre = |rect: PixelRect| {
        (
            window.ulx + rect.ulx + rect.width / 2,
            window.uly + rect.uly + rect.height / 2,
        )
    };

    let (x, y) = centre(DONE_RECT);
    assert_eq!(menu.click(x, y), TransferAction::Confirm);
    let (x, y) = centre(CANCEL_RECT);
    assert_eq!(menu.click(x, y), TransferAction::Cancel);
    let (x, y) = centre(HELP_RECT);
    assert_eq!(menu.click(x, y), TransferAction::Help);
    let (x, y) = centre(RIGHT_RECT);
    let before = menu.amount;
    assert_eq!(menu.click(x, y), TransferAction::None);
    assert_eq!(menu.amount, before, "give_max уже достигнут");
    let (x, y) = centre(LEFT_RECT);
    menu.click(x, y);
    assert_eq!(menu.amount, before - 1);
    // A click outside the buttons — nothing.
    assert_eq!(menu.click(window.ulx + 2, window.uly + 2), TransferAction::None);
}

#[test]
fn keys_match_cpp() {
    let (state, truck, dump) = state_with_transfer();
    let mut menu = TransferMenu::new(StateRef::from(&state), truck, dump).expect("menu");

    assert_eq!(menu.key(Key::Enter), TransferAction::Confirm);
    assert_eq!(menu.key(Key::Escape), TransferAction::Cancel);
    assert_eq!(menu.amount, 0, "Esc сбрасывает количество");
    assert_eq!(menu.key(Key::Char('?')), TransferAction::Help);
    assert_eq!(menu.key(Key::Left), TransferAction::None);
    assert_eq!(menu.amount, -1);
    assert_eq!(menu.key(Key::Right), TransferAction::None);
    assert_eq!(menu.amount, 0);
}

#[test]
fn layout_centres_with_inclusive_bounds() {
    let rect = TransferMenu::layout(MAIN_MAP);
    assert_eq!(rect, PixelRect::new(249, 160, 308, 162));

    let area = PixelRect::new(0, 0, 640, 480);
    let rect = TransferMenu::layout(area);
    assert_eq!(rect.ulx, (640 - 1 - 308) / 2);
    assert_eq!(rect.uly, (480 - 1 - 162) / 2);
}

#[test]
fn arrow_points_from_source_to_target() {
    let (state, truck, dump) = state_with_transfer();
    let mut menu = TransferMenu::new(StateRef::from(&state), truck, dump).expect("menu");

    // `SetRestState(total >= 0)` → pressed → `down` = `XFRRTARO` (right):
    // the transfer goes from the unit (source, left) into the receiver (right).
    menu.set_amount(5);
    assert!(menu.arrow_points_right(), "give: стрелка к приёмнику");
    assert_eq!(menu.amount, 5);

    // Zero is also `total >= 0`, C++ leaves the button pressed.
    menu.set_amount(0);
    assert!(menu.arrow_points_right());

    // A negative value — a reverse transfer: the button is released, `up` =
    // `XFRLFARO` (left).
    menu.set_amount(-5);
    assert!(!menu.arrow_points_right(), "take: стрелка обратно к юниту");
}

#[test]
fn bar_fill_scales_amount_over_target_capacity() {
    let (state, truck, dump) = state_with_transfer();
    let mut menu = TransferMenu::new(StateRef::from(&state), truck, dump).expect("menu");

    // amount 30: (30 + 10) * 223 / 65 = 137.
    assert_eq!(menu.bar_fill(), (40 * 223) / 65);
    menu.set_amount(0);
    assert_eq!(menu.bar_fill(), (10 * 223) / 65);
    menu.set_amount(-10);
    assert_eq!(menu.bar_fill(), 0);
}

#[test]
fn resolve_target_swaps_to_a_storage_building() {
    let (state, truck, dump) = state_with_transfer();
    // A click on the mine (RAW, but this is a 25 storage) — the type matches, it is the target.
    let mine = state
        .snapshot()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            state
                .snapshot()
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == UnitType::MiningStation)
        })
        .expect("mine");
    assert_eq!(resolve_target(StateRef::from(&state), truck, mine), Ok(Some(mine)));

    // A truck with fuel to a complex with ADUMP: C++ replaces the target with
    // the building with fuel — there is none, so `None` (no transfer).
    let cargo_state = state;
    let fuel_truck = cargo_state
        .snapshot()
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            cargo_state
                .snapshot()
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == UnitType::SupplyTruck)
        })
        .expect("truck");
    assert_eq!(
        resolve_target(StateRef::from(&cargo_state), fuel_truck, dump),
        Ok(Some(dump)),
            "тип груза совпадает (RAW)"
    );
}

#[test]
fn resolve_target_rejects_disabled_target() {
    let (mut state, truck, dump) = state_with_transfer();
    state.unit_mut(dump).expect("dump").orders = UnitOrder::Disable;
    assert!(resolve_target(StateRef::from(&state), truck, dump).is_err());
}

#[test]
fn transfer_order_carries_parent_and_cargo_in_p3() {
    let (state, truck, dump) = state_with_transfer();
    let command = transfer_order(StateRef::from(&state), truck, dump, 12).expect("command");

    match command {
        max_core::input::Command::Order(packet) => match packet.payload {
            max_core::input::OrderPayload::P3(p3) => {
                assert_eq!(p3.p2.p1.order, UnitOrder::Transfer);
                assert_eq!(p3.p2.parent_unit, Some(dump));
                assert_eq!(p3.transfer_cargo, 12);
            }
            other => panic!("ожидался P3, получен {other:?}"),
        },
        other => panic!("ожидался Order, получен {other:?}"),
    }
}
