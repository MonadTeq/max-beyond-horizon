//! P6: purchasable upgrades screen — the chain UI → `TeamUnits` → new units.

use max_core::enums::{TeamClan, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_app::upgrademenu::{UpgradeFilter, UpgradeMenu};
use max_app::{App, AppConfig};

fn app_with_team() -> App {
    let mut app = App::new(AppConfig::default());
    let mut state = max_core::universe::GameUniverse::default();
    state.set_player_team(TeamId::RED);
    state.set_active_turn_team(TeamId::RED);
    state
        .team_mut(TeamId::RED)
        .expect("RED slot")
        .team_type = TeamKind::Player;
    *state.team_units_mut(TeamId::RED).expect("RED economy") =
        TeamUnits::with_clan(TeamClan::Random);
    state
        .team_units_mut(TeamId::RED)
        .expect("RED economy")
        .gold = 500;
    app.universe = state;
    app
}

fn select_constructor(app: &mut App) -> max_core::unit::UnitId {
    let id = app
        .universe
        .spawn_unit(
            UnitType::Constructor,
            TeamId::RED,
            Point::new(10, 10),
            0,
        )
        .expect("constructor");
    app.hud_mut().select(Some(id));
    id
}

#[test]
fn upgrade_menu_opens_for_selected_own_unit() {
    let mut app = app_with_team();
    assert!(!app.open_upgrade_menu(), "без выделения меню не открывается");
    select_constructor(&mut app);
    assert!(app.open_upgrade_menu());
    assert!(app.upgrade_menu().is_some());
    assert_eq!(app.upgrade_menu().unwrap().team, TeamId::RED);
    assert!(!app.upgrade_menu().unwrap().unit_types.is_empty());

    app.close_upgrade_menu();
    assert!(app.upgrade_menu().is_none());
}

#[test]
fn upgrade_changes_current_values_and_deducts_gold_on_commit() {
    let mut app = app_with_team();
    select_constructor(&mut app);
    assert!(app.open_upgrade_menu());

    // «Infantry» — a combat type, row 0 — attack.
    let infantry_index = app
        .upgrade_menu()
        .unwrap()
        .unit_types
        .iter()
        .position(|unit_type| *unit_type == UnitType::Infantry)
        .expect("Infantry в списке апгрейдов");
    assert!(app.upgrade_menu_select(infantry_index));

    let attack_before = app
        .state_ref()
        .team_units()[TeamId::RED.0 as usize]
        .current_values[UnitType::Infantry as usize]
        .attack;
    let gold_before = app.state_ref().team_units()[TeamId::RED.0 as usize].gold;

    assert!(app.upgrade_menu_adjust(0, true), "атака покупается");
    app.commit_upgrade_menu(false).expect("commit");

    let attack_after = app.state_ref().team_units()[TeamId::RED.0 as usize].current_values
        [UnitType::Infantry as usize]
        .attack;
    let gold_after = app.state_ref().team_units()[TeamId::RED.0 as usize].gold;
    assert!(
        attack_after > attack_before,
        "апгрейд атаки: {attack_before} -> {attack_after}"
    );
    assert!(gold_after < gold_before, "золото списано: {gold_after}");

    // A new unit of this type is created with the current values (C++
    // `UnitInfo::UnitInfo` → `GetCurrentUnitValues`).
    let spawned = app
        .universe
        .spawn_unit(UnitType::Infantry, TeamId::RED, Point::new(20, 20), 0)
        .expect("infantry");
    assert_eq!(
        app.state_ref().unit(spawned).unwrap().base_values.attack,
        attack_after
    );
}

#[test]
fn cancel_rolls_back_gold_and_values() {
    let mut app = app_with_team();
    select_constructor(&mut app);
    assert!(app.open_upgrade_menu());
    let index = app
        .upgrade_menu()
        .unwrap()
        .unit_types
        .iter()
        .position(|unit_type| *unit_type == UnitType::Infantry)
        .unwrap();
    app.upgrade_menu_select(index);

    let before = app.state_ref().team_units()[TeamId::RED.0 as usize].current_values
        [UnitType::Infantry as usize];
    let gold = app.state_ref().team_units()[TeamId::RED.0 as usize].gold;

    assert!(app.upgrade_menu_adjust(0, true));
    // The menu changes only its own copy of gold.
    assert_eq!(app.state_ref().team_units()[TeamId::RED.0 as usize].gold, gold);
    app.close_upgrade_menu();

    assert_eq!(
        app.state_ref().team_units()[TeamId::RED.0 as usize].current_values
            [UnitType::Infantry as usize],
        before
    );
    assert_eq!(app.state_ref().team_units()[TeamId::RED.0 as usize].gold, gold);
}

#[test]
fn filters_keep_only_the_matching_unit_class() {
    let mut app = app_with_team();
    select_constructor(&mut app);
    let menu = UpgradeMenu::new(app.state_ref(), TeamId::RED).expect("menu");
    assert!(menu.unit_types.iter().any(|unit_type| {
        max_core::access::unit_type_flags(*unit_type) & max_core::enums::unit_flags::STATIONARY != 0
    }));

    let mut menu = menu;
    menu.filters = [true, false, false, false, false];
    menu.rebuild_list(app.state_ref(), None);
    assert!(!menu.unit_types.is_empty());
    assert!(menu.unit_types.iter().all(|unit_type| {
        max_core::access::unit_type_flags(*unit_type) & max_core::enums::unit_flags::MOBILE_LAND_UNIT
            != 0
    }));

    menu.toggle_filter(UpgradeFilter::Ground, app.state_ref());
    assert!(menu.filters[0] == false);
}

#[test]
fn upgrade_menu_is_modal_for_session_input() {
    use max_app::ui::{handle_session_input, InputEvent, Key, Transition};

    let mut app = app_with_team();
    select_constructor(&mut app);
    assert!(app.open_upgrade_menu());

    // Esc closes the menu but does not leave the match.
    let transition = handle_session_input(&mut app, InputEvent::Key(Key::Escape), false);
    assert_eq!(transition, Transition::None);
    assert!(app.upgrade_menu().is_none());

    // A click on the map while the menu is open does not change the selection (the menu is modal).
    assert!(app.open_upgrade_menu());
    let selected = app.hud().selected;
    let transition = handle_session_input(&mut app, InputEvent::MouseClick(300, 300), false);
    assert_eq!(transition, Transition::None);
    assert_eq!(app.hud().selected, selected);
}
