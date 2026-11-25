use super::*;
use max_core::TeamUnits;

fn state() -> max_core::universe::GameUniverse<max_core::universe::LocalDispatcher> {
    let mut state = max_core::universe::GameUniverse::default();
    state
        .team_mut(TeamId::RED)
        .expect("RED slot")
        .team_type = max_core::TeamKind::Player;
    let team_units = state.team_units_mut(TeamId::RED).expect("RED economy");
    *team_units = TeamUnits::with_clan(max_core::TeamClan::Random);
    team_units.set_gold(150);
    state
}

/// The UI shows the UFN, not the former `unit_name` strings.
///
/// 9 renamed and 3 previously missing types (`MonopoleMine`,
/// `SupportPlant`, `RecreationCenter` were returned as "Unit") verified against
/// `data/unit_names.json` (`max-brh-docs/unit-name-review.md`).
#[test]
fn unit_name_is_the_user_friendly_name() {
    for unit_type in UnitType::ALL {
        assert_eq!(
            unit_name(*unit_type),
            crate::unit_names::unit_user_friendly_name(*unit_type),
                "{unit_type:?}: UI-строка должна совпадать с UFN-таблицей"
        );
    }

    for (unit_type, expected) in [
        (UnitType::GoldRefinery, "Gold Refinery"),
        (UnitType::ResearchCentre, "Research Centre"),
        (UnitType::EcoSphere, "Eco-Sphere"),
        (UnitType::MaterialStorage, "Material Storage"),
        (UnitType::FuelStorage, "Fuel Storage"),
        (UnitType::AntiAir, "Anti-Air"),
        (UnitType::Artillery, "Artillery"),
        (UnitType::MobileAntiAir, "Mobile Anti-Air"),
        (
            UnitType::ArmouredPersonnelCarrier,
                "Armoured Personnel Carrier",
        ),
        (UnitType::MonopoleMine, "Monopole Mine"),
        (UnitType::SupportPlant, "Support Plant"),
        (UnitType::RecreationCenter, "Recreation Center"),
    ] {
        assert_eq!(unit_name(unit_type), expected, "{unit_type:?}");
    }
}

#[test]
fn mobile_menu_lists_buildings_and_queues_one() {
    let mut state = state();
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    let mut menu = BuildMenu::new(StateRef::from(&state), constructor).expect("constructor builds");

    assert!(!menu.factory);
    assert_eq!(menu.unit_types.len(), 15);
    assert_eq!(menu.selected_type(), Some(UnitType::MiningStation));

    // Selection of TRAINHAL (index 13 in the CONSTRCT list).
    let index = menu
        .unit_types
        .iter()
        .position(|unit| *unit == UnitType::TrainingHall)
        .expect("training hall buildable");
    menu.select(index);
    assert!(menu.add_to_queue());
    assert_eq!(menu.queue, vec![UnitType::TrainingHall]);

    // The second selection replaces (MobileBuildMenu::Build: Clear + PushBack).
    menu.select(0);
    assert!(menu.add_to_queue());
    assert_eq!(menu.queue, vec![UnitType::MiningStation]);

    let command = menu.command(StateRef::from(&state)).expect("build command");
    let Command::Order(packet) = command else {
        panic!("expected order");
    };
    let OrderPayload::P4(p4) = packet.payload else {
        panic!("expected P4 payload");
    };
    assert_eq!(p4.p3.p2.p1.order, UnitOrder::Build);
    assert_eq!(p4.build_list, vec![UnitType::MiningStation]);
    assert_eq!(p4.build_time, 12, "MININGST: ATTRIB_TURNS = 12");
    assert_ne!(p4.p3.move_to, Point::new(10, 10), "площадка не под юнитом");
}

#[test]
fn factory_menu_builds_a_queue_and_repeats() {
    let mut state = state();
    let factory = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
        .expect("factory");
    let mut menu = BuildMenu::new(StateRef::from(&state), factory).expect("factory builds");

    assert!(menu.factory);
    assert_eq!(menu.unit_types.len(), 2);
    menu.select(1); // INFANTRY
    assert!(menu.add_to_queue());
    menu.select(0); // COMMANDO
    assert!(menu.add_to_queue());
    assert_eq!(
        menu.queue,
        vec![UnitType::Infantry, UnitType::Infiltrator],
            "порядок очереди — порядок добавления"
    );

    assert!(menu.toggle_repeat());
    let command = menu.command(StateRef::from(&state)).expect("command");
    let Command::Order(packet) = command else {
        panic!("expected order");
    };
    let OrderPayload::P4(p4) = packet.payload else {
        panic!("expected P4 payload");
    };
    assert!(p4.repeat_build);
    assert_eq!(p4.build_list, menu.queue);
    assert_eq!(p4.p3.move_to, Point::new(10, 10), "завод строит на месте");
}

#[test]
fn cancel_closes_without_a_command() {
    let mut state = state();
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    let mut menu = BuildMenu::new(StateRef::from(&state), constructor).expect("menu");

    assert_eq!(
        menu.click(StateRef::from(&state), CANCEL.ulx + 1, CANCEL.uly + 1),
        BuildMenuAction::Close
    );
    assert_eq!(
        menu.key(StateRef::from(&state), crate::ui::Key::Escape),
        BuildMenuAction::Close
    );
    // Cancel alone does not change the selection: the order exists only after
    // `Done` (C++ `event_click_cancel` does not call `Build()`).
    assert!(menu.queue.is_empty());
    let command = menu.command(StateRef::from(&state)).expect("выбранный тип — это заказ");
    let Command::Order(packet) = command else {
        panic!("expected order");
    };
    let OrderPayload::P4(p4) = packet.payload else {
        panic!("expected P4 payload");
    };
    assert_eq!(p4.build_list, vec![menu.selected_type().expect("selected")]);
}

#[test]
fn rows_scroll_with_the_arrows() {
    let mut state = state();
    let constructor = state
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    let mut menu = BuildMenu::new(StateRef::from(&state), constructor).expect("menu");

    for _ in 0..LIST_ROWS {
        menu.page_list(true);
    }

    assert_eq!(menu.selected, LIST_ROWS);
    assert!(menu.page_min <= menu.selected);
    assert_eq!(
        menu.click(StateRef::from(&state), LIST_UP.ulx + 1, LIST_UP.uly + 1),
        BuildMenuAction::None
    );
    assert_eq!(menu.selected, LIST_ROWS - 1);

    let down_row = 3;
    menu.click(
        StateRef::from(&state),
        LIST_AREA.ulx + 2,
        LIST_AREA.uly + down_row * LIST_ROW + 2,
    );
    assert_eq!(menu.selected, menu.page_min + down_row as usize);
}

#[test]
fn non_builders_have_no_menu() {
    let mut state = state();
    let tank = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
        .expect("tank");

    assert!(BuildMenu::new(StateRef::from(&state), tank).is_none());
}

#[test]
fn rate_numbers_are_turns_and_total_cost_like_get_turns_to_build() {
    let mut state = state();
    let factory = state
        .spawn_unit(
            UnitType::HeavyVehiclePlant,
            TeamId::RED,
            Point::new(10, 10),
            0,
        )
        .expect("factory");
    let mut menu = BuildMenu::new(StateRef::from(&state), factory).expect("factory menu");
    let tank = menu
        .unit_types
        .iter()
        .position(|unit| *unit == UnitType::Tank)
        .expect("TANK in HVP list");
    menu.select(tank);

    // TANK = 4 turns; factory 3/turn at 1×, 12 at 2×, 36 at 4×. The second number —
    // the total cost (sum of turns), not the rate per turn.
    assert_eq!(menu.max_rate(StateRef::from(&state)), 4);
    assert_eq!(menu.rate_numbers(StateRef::from(&state), 1), (4, 12));
    assert_eq!(menu.rate_numbers(StateRef::from(&state), 2), (2, 24));
    assert_eq!(menu.rate_numbers(StateRef::from(&state), 4), (1, 36));
}

#[test]
fn training_hall_is_locked_to_1x_like_the_cpp_buttons() {
    let mut state = state();
    let hall = state
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(10, 10), 0)
        .expect("training hall");
    let mut menu = BuildMenu::new(StateRef::from(&state), hall).expect("factory menu");
    let infantry = menu
        .unit_types
        .iter()
        .position(|unit| *unit == UnitType::Infantry)
        .expect("INFANTRY in TRAINHAL list");
    menu.select(infantry);

    // C++ `AbstractBuildMenu::Draw`: `build_rate_x2/x4->Enable(false)`.
    assert_eq!(menu.max_rate(StateRef::from(&state)), 1);
    // INFANTRY = 2 turns, TRAINHAL rate 1/turn.
    assert_eq!(menu.rate_numbers(StateRef::from(&state), 1), (2, 2));
}

#[test]
fn clan_turns_change_the_build_menu_cost() {
    let mut random = state();
    let builder = random
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(10, 10), 0)
        .expect("constructor");
    // In a real match `deploy_starting_units` sets `storage = cargo`.
    random.unit_mut(builder).unwrap().storage = 0;
    let mut menu = BuildMenu::new(StateRef::from(&random), builder).expect("menu");
    let shipyard = menu
        .unit_types
        .iter()
        .position(|unit| *unit == UnitType::Shipyard)
        .expect("SHIPYARD in CONSTRCT list");
    menu.select(shipyard);
    assert_eq!(menu.max_rate(StateRef::from(&random)), 1);
    assert_eq!(menu.rate_numbers(StateRef::from(&random), 1), (20, 40));

    // CrimsonPath: SHIPYARD -5 turns (`CLAN_TRADEOFFS`), price 15 × 2.
    *random.team_units_mut(TeamId::RED).expect("RED economy") =
        TeamUnits::with_clan(max_core::TeamClan::CrimsonPath);
    random.unit_mut(builder).unwrap().storage = 0;
    let mut menu = BuildMenu::new(StateRef::from(&random), builder).expect("menu");
    menu.select(shipyard);
    assert_eq!(menu.rate_numbers(StateRef::from(&random), 1), (15, 30));
}

#[test]
fn factory_rate_numbers_follow_the_active_selection_not_the_queue_head() {
    let mut state = state();
    let factory = state
        .spawn_unit(
            UnitType::HeavyVehiclePlant,
            TeamId::RED,
            Point::new(10, 10),
            0,
        )
        .expect("factory");
    let mut menu = BuildMenu::new(StateRef::from(&state), factory).expect("factory menu");
    let tank = menu
        .unit_types
        .iter()
        .position(|unit| *unit == UnitType::Tank)
        .expect("TANK in HVP list");
    let launcher = menu
        .unit_types
        .iter()
        .position(|unit| *unit == UnitType::RocketLauncher)
        .expect("ROCKET in HVP list");

    // TANK is in the queue, but ROCKET is selected: C++ `Draw(selector->GetLast())`
    // shows the numbers of the selected row (4 TANK turns — only when
    // the queue is active).
    menu.queue.push(UnitType::Tank);
    menu.select(launcher);
    let turns = menu.turns_for(StateRef::from(&state), UnitType::RocketLauncher);
    assert_eq!(menu.rate_numbers(StateRef::from(&state), 1), (turns, turns * 3));

    menu.queue_selected = 0;
    menu.queue_active = true;
    assert_eq!(menu.rate_numbers(StateRef::from(&state), 1), (4, 12));
    assert_eq!(menu.rate_numbers(StateRef::from(&state), 2), (2, 24));
    assert_eq!(menu.rate_numbers(StateRef::from(&state), 4), (1, 36));

    // Back to the list — the selected row again.
    menu.queue_active = false;
    menu.select(tank);
    assert_eq!(menu.rate_numbers(StateRef::from(&state), 1), (4, 12));
}

#[test]
fn stats_rows_for_a_building_carry_icons_and_the_cpp_cost() {
    let state = state();
    let team_units = &StateRef::from(&state).team_units()[0];
    // `AbstractBuildMenu::Draw` (`buildmenu.cpp:536`): I_RAW/I_RAWE.
    let rows = crate::mobilize::Mobilization::stats_rows_for(
        &team_units.base_values,
        &team_units.current_values,
        UnitType::TrainingHall,
            "I_RAW",
            "I_RAWE",
    );
    let labels: Vec<&str> = rows.iter().map(|row| row.label).collect();

    assert!(labels.contains(&"Hits"), "характеристики: {labels:?}");
    assert!(labels.contains(&"Armor"), "характеристики: {labels:?}");
    assert_eq!(labels.last(), Some(&"Cost"), "строка Cost последняя");

    // `UnitStats_DrawCost`: CONSTRCT builder rate (2) × ATTRIB_TURNS (10);
    // the pips are raw materials, not the mobilization (gold) price.
    let cost = rows.last().expect("cost row");
    assert_eq!(cost.value, Some(20));
    assert_eq!(cost.icons.len(), 1);
    assert_eq!(cost.icons[0].resource, "I_RAW");
    assert_eq!(cost.icons[0].empty_resource, "I_RAWE");
    assert_eq!(cost.icons[0].current, 20);
}
