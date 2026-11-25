use super::*;
use max_core::enums::{unit_flags, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::Complex;
use max_core::TeamUnits;

fn state_with_mine(
    complex_id: i16,
) -> (
    max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
    UnitId,
    ComplexId,
) {
    let mut state = max_core::universe::GameUniverse::default();
    state
        .team_mut(TeamId::RED)
        .expect("RED slot")
        .team_type = max_core::team::TeamKind::Player;
    *state.team_units_mut(TeamId::RED).expect("RED economy") =
        TeamUnits::with_clan(max_core::enums::TeamClan::Random);

    // A 2×2 square around the mine: raw 8+8 (measurement limit 16), fuel 8,
    // gold 8 — like `Survey_GetResourcesInArea(..., 1, 16, ...)`.
    let resources = [
        (20, 20, 8, max_core::map::ResourceKind::Raw),
        (21, 20, 8, max_core::map::ResourceKind::Raw),
        (20, 21, 8, max_core::map::ResourceKind::Gold),
        (21, 21, 8, max_core::map::ResourceKind::Fuel),
    ];
    for (x, y, amount, kind) in resources {
        state.map_mut().resources.set(
            x,
            y,
            max_core::map::ResourceMapEntry::new(amount, Some(kind), 0),
        );
    }

    // Control data, not classification reading: the mask is an argument of
    // `spawn_unit` (instance flags), so `STATIONARY | BUILDING`
    // remain raw bits.
    let flags = unit_flags::STATIONARY | unit_flags::BUILDING;
    let id = state
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(20, 20), flags)
        .expect("шахта");

    let complex = ComplexId(complex_id);
    if let Some(units) = state.team_units_mut(TeamId::RED) {
        units.complexes.push(Complex::new(complex_id));
    }

    let mut unit = state.snapshot().unit(id).cloned().expect("unit");
    unit.orders = UnitOrder::PowerOn;
    unit.state = UnitOrderState::ExecutingOrder;
    unit.complex = Some(complex);
    // `spawn_unit` applies `UnitsManager_SetInitialMining` (C++ `DeployUnit`),
    // so the mine starts pre-allocated. The dialog tests need a neutral
    // 0/0/0 split to check the absolute bar/arrow mechanics.
    unit.raw_mining = 0;
    unit.fuel_mining = 0;
    unit.gold_mining = 0;
    unit.total_mining = 0;
    state.write_unit(id, unit);

    (state, id, complex)
}

#[test]
fn layout_matches_the_oracle() {
    assert_eq!(WINDOW_RECT, PixelRect::new(0, 0, 640, 480));
    assert_eq!(TITLE_RECT, PixelRect::new(230, 6, 180, 20));
    assert_eq!(LEFT_ARROW_X, 139);
    assert_eq!(RIGHT_ARROW_X, 421);
    assert_eq!(ARROW_SIZE, (28, 29));
    assert_eq!(SECTION_TOP, [70, 190, 310]);
    assert_eq!(BAR_RECT, PixelRect::new(174, 0, 240, 30));
    assert_eq!(BAR_STEP, 37);
    assert_eq!(HELP_RECT, PixelRect::new(465, 438, 48, 40));
    assert_eq!(DONE_RECT, PixelRect::new(514, 438, 109, 40));
    // The tenth C++ label is empty (a table of 10 elements).
    assert_eq!(LABEL_RECTS.len(), TITLE_COUNT);
    assert!(LABEL_RECTS[9].is_empty());
    assert_eq!(LABEL_RECTS[0], PixelRect::new(42, 74, 78, 19));
    assert_eq!(LABEL_RECTS[8], PixelRect::new(42, 389, 78, 18));
}

#[test]
fn labels_match_language_json() {
    assert_eq!(LABELS[0], "Raw");
    assert_eq!(LABELS[1], "Usage");
    assert_eq!(LABELS[2], "Reserve");
    assert_eq!(LABELS[3], "Fuel");
    assert_eq!(LABELS[6], "Gold");
}

#[test]
fn opening_requires_an_active_mine() {
    let (mut state, id, _complex) = state_with_mine(1);
    assert!(AllocMenu::new(&mut state, id).is_some());

    state.unit_mut(id).unwrap().orders = UnitOrder::PowerOff;
    assert!(
        AllocMenu::new(&mut state, id).is_none(),
            "выключенная шахта не открывает распределение (в C++ пункт тогда `start`)"
    );

    state.unit_mut(id).unwrap().orders = UnitOrder::PowerOn;
    state.unit_mut(id).unwrap().complex = None;
    assert!(AllocMenu::new(&mut state, id).is_none(), "нет комплекса");
}

#[test]
fn opening_reports_mining_capacity_from_the_cell() {
    let (mut state, id, _complex) = state_with_mine(1);
    let menu = AllocMenu::new(&mut state, id).expect("меню");

    assert_eq!(menu.production_of(MiningKind::Raw), 0);
    assert!(
        menu.capacity_of(MiningKind::Raw) >= 0,
            "максимум клетки неотрицателен"
    );
    assert_eq!(menu.free_capacity(), 16);
    assert_eq!(menu.production.free_capacity, 16);
}

#[test]
fn increase_and_decrease_move_the_mine_counters() {
    let (mut state, id, _complex) = state_with_mine(1);
    let mut menu = AllocMenu::new(&mut state, id).expect("меню");

    assert!(!(menu.capacity_of(MiningKind::Raw) == 0), concat!("required test fixture is missing: ", stringify!(menu.capacity_of(MiningKind::Raw) == 0)));

    assert!(menu.increase(&mut state, MiningKind::Raw));
    assert_eq!(menu.production_of(MiningKind::Raw), 1);
    assert_eq!(state.snapshot().unit(id).unwrap().raw_mining, 1);
    assert_eq!(state.snapshot().unit(id).unwrap().total_mining, 1);
    assert_eq!(menu.free_capacity(), 15);

    assert!(menu.decrease(&mut state, MiningKind::Raw));
    assert_eq!(menu.production_of(MiningKind::Raw), 0);
    assert_eq!(state.snapshot().unit(id).unwrap().raw_mining, 0);
    assert_eq!(state.snapshot().unit(id).unwrap().total_mining, 0);
}

#[test]
fn level_change_is_absolute_like_a_bar_click() {
    let (mut state, id, _complex) = state_with_mine(1);
    let mut menu = AllocMenu::new(&mut state, id).expect("меню");
    let capacity = menu.capacity_of(MiningKind::Raw);

    assert!(!(capacity < 3), concat!("required test fixture is missing: ", stringify!(capacity < 3)));

    menu.set_level(&mut state, MiningKind::Raw, 3);
    assert_eq!(menu.production_of(MiningKind::Raw), 3);
    assert_eq!(state.snapshot().unit(id).unwrap().raw_mining, 3);

    menu.set_level(&mut state, MiningKind::Raw, 1);
    assert_eq!(menu.production_of(MiningKind::Raw), 1);
    assert_eq!(state.snapshot().unit(id).unwrap().raw_mining, 1);
}

#[test]
fn bar_click_value_matches_cpp_formula() {
    let (mut state, id, _complex) = state_with_mine(1);
    let menu = AllocMenu::new(&mut state, id).expect("меню");
    let capacity = menu.capacity_of(MiningKind::Raw);

    // `x -= bounds.ulx + 174; return (capacity * x + 120) / 240;`
    assert_eq!(menu.bar_click_value(MiningKind::Raw, 174), 0);
    assert_eq!(
        menu.bar_click_value(MiningKind::Raw, 174 + 240),
        (capacity * 240 + 120) / 240
    );
    assert_eq!(
        menu.bar_click_value(MiningKind::Raw, 174 + 120),
        (capacity * 120 + 120) / 240
    );
}

#[test]
fn clicks_hit_the_expected_targets() {
    let (mut state, id, _complex) = state_with_mine(1);
    let mut menu = AllocMenu::new(&mut state, id).expect("меню");

    // `?` and `Done`.
    assert_eq!(
        menu.click(&mut state, HELP_RECT.ulx + 1, HELP_RECT.uly + 1),
        AllocAction::Help
    );
    assert_eq!(
        menu.click(&mut state, DONE_RECT.ulx + 1, DONE_RECT.uly + 1),
        AllocAction::Close
    );

    // A click outside — nothing.
    assert_eq!(
        menu.click(&mut state, 320, 200),
        AllocAction::None,
            "полоса не меняет приказ"
    );

    // Right arrow of Raw.
    let before = menu.production_of(MiningKind::Raw);
    menu.click(&mut state, RIGHT_ARROW_X + 2, SECTION_TOP[0] + 2);
    let after = menu.production_of(MiningKind::Raw);
    assert!(after == before || after == before + 1);

    // The left arrow returns.
    menu.click(&mut state, LEFT_ARROW_X + 2, SECTION_TOP[0] + 2);
    assert_eq!(menu.production_of(MiningKind::Raw), before);
}

#[test]
fn keys_close_select_and_adjust() {
    let (mut state, id, _complex) = state_with_mine(1);
    let mut menu = AllocMenu::new(&mut state, id).expect("меню");

    assert_eq!(menu.key(&mut state, Key::Escape), AllocAction::Close);
    assert_eq!(menu.key(&mut state, Key::Enter), AllocAction::Close);
    assert_eq!(menu.key(&mut state, Key::Char('h')), AllocAction::Help);

    assert_eq!(menu.selected, MiningKind::Raw);
    menu.key(&mut state, Key::Down);
    assert_eq!(menu.selected, MiningKind::Fuel);
    menu.key(&mut state, Key::Down);
    assert_eq!(menu.selected, MiningKind::Gold);
    menu.key(&mut state, Key::Down);
    assert_eq!(menu.selected, MiningKind::Raw);
    menu.key(&mut state, Key::Up);
    assert_eq!(menu.selected, MiningKind::Gold);

    let before = menu.production_of(MiningKind::Fuel);
    menu.key(&mut state, Key::Right);
    let after = menu.production_of(MiningKind::Fuel);
    assert!(after == before || after == before + 1);
    menu.key(&mut state, Key::Left);
    assert_eq!(menu.production_of(MiningKind::Fuel), before);
}

#[test]
fn bar_captions_follow_cpp_format() {
    let (mut state, id, _complex) = state_with_mine(1);
    let mut menu = AllocMenu::new(&mut state, id).expect("меню");

    assert!(!(menu.capacity_of(MiningKind::Raw) < 2), concat!("required test fixture is missing: ", stringify!(menu.capacity_of(MiningKind::Raw) < 2)));

    menu.set_level(&mut state, MiningKind::Raw, 2);
    assert_eq!(menu.bar_caption(0, 0), "2");
    // `%i (%+i / turn)` — `usage` and `production − usage`.
    assert!(menu.bar_caption(0, 1).contains("/ turn"));
    assert!(menu.bar_caption(0, 1).starts_with(&menu.usage_of(MiningKind::Raw).to_string()));
    assert_eq!(menu.bar_caption(0, 2), menu.stored_of(MiningKind::Raw).to_string());
}

#[test]
fn bar_geometry_follows_cpp() {
    let (mut state, id, _complex) = state_with_mine(1);
    let mut menu = AllocMenu::new(&mut state, id).expect("меню");
    let capacity = menu.capacity_of(MiningKind::Raw);

    assert!(!(capacity < 4), concat!("required test fixture is missing: ", stringify!(capacity < 4)));

    menu.set_level(&mut state, MiningKind::Raw, 4);
    // Fill `production * 240 / max(capacity, usage)`.
    let scale = capacity.max(menu.usage_of(MiningKind::Raw));
    assert_eq!(menu.bar_fill(0, 0), (4 * 240 / scale).clamp(0, 240));
    // The second and third bars without a "tape".
    assert_eq!(menu.bar_tape(0, 1), 0);
    assert_eq!(menu.bar_tape(0, 2), 0);
}

#[test]
fn determinism_of_two_identical_dialogs() {
    let (mut a, id_a, _) = state_with_mine(1);
    let (mut b, id_b, _) = state_with_mine(1);

    let mut menu_a = AllocMenu::new(&mut a, id_a).expect("меню");
    let mut menu_b = AllocMenu::new(&mut b, id_b).expect("меню");

    for _ in 0..3 {
        menu_a.increase(&mut a, MiningKind::Raw);
        menu_b.increase(&mut b, MiningKind::Raw);
    }
    menu_a.decrease(&mut a, MiningKind::Raw);
    menu_b.decrease(&mut b, MiningKind::Raw);

    assert_eq!(menu_a.production, menu_b.production);
    assert_eq!(menu_a.balance, menu_b.balance);
    assert_eq!(
        a.snapshot().unit(id_a).unwrap().raw_mining,
        b.snapshot().unit(id_b).unwrap().raw_mining
    );
    assert_eq!(
        a.snapshot().unit(id_a).unwrap().total_mining,
        b.snapshot().unit(id_b).unwrap().total_mining
    );
    // The state digest (CRC/snapshot) matches too: the value is part of the
    // snapshot via the unit fields.
    assert_eq!(
        a.snapshot().state_checksum(),
        b.snapshot().state_checksum()
    );
    assert_eq!(a.snapshot().snapshot_bytes(), b.snapshot().snapshot_bytes());
}
