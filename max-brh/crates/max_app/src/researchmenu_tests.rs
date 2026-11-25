/// Applies the menu commit the way the command channel does: the menu returns the
/// committed allocation, packet 09 is built with it and applied by
/// `events::apply_packet_09`; the resulting allocation is returned.
fn commit(
    menu: &mut ResearchMenu,
    mut universe: max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
) -> (
    max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
    [u32; RESEARCH_TOPIC_COUNT],
) {
    let target = menu.commit();
    let team = menu.team;
    let mut research = universe.snapshot().teams()[team.0 as usize].research_topics;

    for (topic, count) in research.iter_mut().zip(target.iter()) {
        topic.allocation = *count;
    }

    let snapshot = universe.snapshot();
    let command = max_core::input::Command::TeamState(max_core::input::Packet09TeamState {
        team,
        gold: snapshot.team_units()[team.0 as usize].gold,
        gold_spent_on_upgrades: snapshot.teams()[team.0 as usize]
            .stats_gold_spent_on_upgrades,
        research,
        name: "RED".to_string(),
    });

    assert_eq!(
        universe.apply_now(command),
        max_core::events::ApplyOutcome::Applied,
            "packet 09 applies"
    );

    let actual = universe.snapshot().research_allocation(team);
    (universe, actual)
}

use super::*;
use max_core::enums::ResearchTopic;
use max_core::geo::Point;
use max_core::research::research_turns_to_complete;
use max_core::team::TeamId;

fn state_with_center(
    topic: ResearchTopic,
    level: u32,
) -> (
    max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
    TeamId,
) {
    let mut state = max_core::universe::GameUniverse::default();
    state
        .team_mut(TeamId::RED)
        .expect("RED slot")
        .research_topics[topic as usize]
        .research_level = level;
    let id = state
        .spawn_unit(UnitType::ResearchCentre, TeamId::RED, Point::new(20, 20), 0)
        .expect("research center");
    let mut unit = state.snapshot().unit(id).cloned().expect("unit");
    unit.orders = UnitOrder::PowerOn;
    unit.research_topic = topic;
    state.write_unit(id, unit);
    state.update_research_progress(TeamId::RED, topic, 1);
    (state, TeamId::RED)
}

#[test]
fn layout_matches_the_oracle() {
    assert_eq!(WINDOW_RECT, PixelRect::new(223, 76, 360, 331));
    // `180 + (448-360)/2 = 224`? C++ integer: (447-360)/2 = 43 → 223.
    assert_eq!(WINDOW_RECT.ulx, 180 + (447 - 360) / 2);
    assert_eq!(WINDOW_RECT.uly, 18 + (447 - 331) / 2);
    assert_eq!(FIRST_ROW_Y, 70);
    assert_eq!(ROW_HEIGHT, 28);
    assert_eq!(
        PixelRect::new(
            LEFT_ARROW.0,
            FIRST_ROW_Y + 7 * ROW_HEIGHT,
            ARROW_SIZE.0,
            ARROW_SIZE.1
        ),
        PixelRect::new(72, 266, 18, 17)
    );
    assert_eq!(
        frame_rect(DONE_RECT),
        PixelRect::new(223 + 194, 76 + 294, 75, 25)
    );
}

#[test]
fn topic_order_matches_cpp() {
    let topics = [
        ResearchTopic::Attack,
        ResearchTopic::Shots,
        ResearchTopic::Range,
        ResearchTopic::Armor,
        ResearchTopic::Hits,
        ResearchTopic::Speed,
        ResearchTopic::Scan,
        ResearchTopic::Cost,
    ];

    for (index, topic) in topics.iter().enumerate() {
        assert_eq!(*topic as usize, index);
    }
    assert_eq!(TOPIC_LABELS[0], "Attack");
    assert_eq!(TOPIC_LABELS[7], "Cost");
    assert_eq!(TOPIC_ICONS[0], "I_HRDATK");
    assert_eq!(TOPIC_ICONS[7], "I_GOLD");
}

#[test]
fn opening_counts_powered_centers_per_topic() {
    let (state, team) = state_with_center(ResearchTopic::Attack, 0);
    let menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");

    assert_eq!(menu.active_centers, 1);
    assert_eq!(menu.values[ResearchTopic::Attack as usize], 1);
    assert_eq!(menu.free_capacity, 0, "в C++ free_capacity стартует с 0");
    assert_eq!(menu.slider_offset(ResearchTopic::Attack as usize), 48 - 14);
}

#[test]
fn can_only_redistribute_released_centers() {
    let (state, team) = state_with_center(ResearchTopic::Attack, 0);
    let mut menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");

    // Cannot go up: there are no free centers.
    assert!(!menu.adjust(ResearchTopic::Shots as usize, true));
    assert_eq!(menu.values[ResearchTopic::Shots as usize], 0);

    // Down on Attack frees a center...
    assert!(menu.adjust(ResearchTopic::Attack as usize, false));
    assert_eq!(menu.free_capacity, 1);
    assert_eq!(menu.values[ResearchTopic::Attack as usize], 0);

    // ...and it can be given to Shots.
    assert!(menu.adjust(ResearchTopic::Shots as usize, true));
    assert_eq!(menu.free_capacity, 0);
    assert_eq!(menu.values[ResearchTopic::Shots as usize], 1);
}

#[test]
fn slider_maps_pixels_to_centers_like_cpp() {
    let (state, team) = state_with_center(ResearchTopic::Attack, 0);
    let menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");
    let left = WINDOW_RECT.ulx + SLIDER_BG.ulx;
    let right = left + SLIDER_BG.width;

    assert_eq!(menu.slider_value(0, left), 0);
    assert_eq!(menu.slider_value(0, right), 1);
}

#[test]
fn done_reassigns_the_unit_and_keeps_allocation_consistent() {
    let (state, team) = state_with_center(ResearchTopic::Attack, 0);
    let mut menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");
    menu.adjust(ResearchTopic::Attack as usize, false);
    menu.adjust(ResearchTopic::Shots as usize, true);
    let (state, actual) = commit(&mut menu, state);

    assert_eq!(actual[ResearchTopic::Attack as usize], 0);
    assert_eq!(actual[ResearchTopic::Shots as usize], 1);

    let allocation = state.snapshot().research_allocation(team);
    assert_eq!(allocation[ResearchTopic::Attack as usize], 0);
    assert_eq!(allocation[ResearchTopic::Shots as usize], 1);

    let unit = state
        .snapshot()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| state.snapshot().unit(id))
        .find(|unit| unit.unit_type == UnitType::ResearchCentre)
        .expect("center");
    assert_eq!(unit.research_topic, ResearchTopic::Shots);
}

#[test]
fn done_with_released_centers_powers_them_off() {
    let (state, team) = state_with_center(ResearchTopic::Attack, 0);
    let mut menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");
    menu.adjust(ResearchTopic::Attack as usize, false);
    let (mut state, actual) = commit(&mut menu, state);

    assert_eq!(
        actual.iter().sum::<u32>(),
        1,
            "центр ещё числится до Power Off"
    );
    let unit = state
        .snapshot()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| state.snapshot().unit(id))
        .find(|unit| unit.unit_type == UnitType::ResearchCentre)
        .expect("center");
    assert_eq!(unit.orders, UnitOrder::PowerOff);

    // On the next tick `process_order_power_off` writes off the allocation
    // (`ResearchMenu_UpdateResearchProgress(-1)`), as in C++.
    state.tick();
    assert_eq!(state.snapshot().research_allocation(team).iter().sum::<u32>(), 0);
}

#[test]
fn turns_to_complete_shrink_with_more_centers() {
    let (mut state, team) = state_with_center(ResearchTopic::Attack, 0);
    let mut menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");
    let one = menu
        .turns_to_complete(StateRef::from(&state), ResearchTopic::Attack as usize)
        .unwrap();
    assert!(one > 0);
    assert_eq!(
        one,
        research_turns_to_complete(0, RESEARCH_TOPIC_FACTORS[0]) as u32
    );

    // Second center: the topic speed changes (a deterministic formula).
    let second = state
        .spawn_unit(UnitType::ResearchCentre, TeamId::RED, Point::new(21, 20), 0)
        .expect("second center");
    let mut unit = state.snapshot().unit(second).cloned().expect("unit");
    unit.orders = UnitOrder::PowerOn;
    unit.research_topic = ResearchTopic::Attack;
    state.write_unit(second, unit);
    state.update_research_progress(TeamId::RED, ResearchTopic::Attack, 1);

    menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");
    assert_eq!(menu.value(ResearchTopic::Attack as usize), 2);
    let two = menu
        .turns_to_complete(StateRef::from(&state), ResearchTopic::Attack as usize)
        .unwrap();
    assert!(two < one, "два центра быстрее одного: {two} < {one}");
}

#[test]
fn research_new_turn_uses_the_same_distribution() {
    let (state, team) = state_with_center(ResearchTopic::Attack, 0);
    let mut menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");
    menu.adjust(ResearchTopic::Attack as usize, false);
    menu.adjust(ResearchTopic::Shots as usize, true);
    let (mut state, _) = commit(&mut menu, state);

    // `finish_turn`/`advance_research` tick exactly the topic to which
    // the centers were redistributed: Attack stays 0, Shots grows.
    for _ in 0..16 {
        state.advance_research(team);
    }
    assert_eq!(
        state.snapshot().teams()[team.0 as usize].research_topics[ResearchTopic::Attack as usize]
            .research_level,
        0
    );
    assert!(
        state.snapshot().teams()[team.0 as usize].research_topics[ResearchTopic::Shots as usize]
            .research_level
            >= 1
    );
}

#[test]
fn action_keys_and_clicks() {
    let (state, team) = state_with_center(ResearchTopic::Attack, 0);
    let mut menu = ResearchMenu::new(StateRef::from(&state), team).expect("menu");

    assert_eq!(menu.key(Key::Enter), ResearchAction::Done);
    assert_eq!(menu.key(Key::Escape), ResearchAction::Cancel);
    assert_eq!(menu.key(Key::Char('h')), ResearchAction::Help);

    // Buttons in frame coordinates: Help (167..198) overlaps the left edge of
    // Done (194..269), so we click the button centers.
    assert_eq!(
        menu.click(WINDOW_RECT.ulx + 92 + 37, WINDOW_RECT.uly + 294 + 12),
        ResearchAction::Cancel
    );
    assert_eq!(
        menu.click(WINDOW_RECT.ulx + 167 + 15, WINDOW_RECT.uly + 294 + 13),
        ResearchAction::Help
    );
    assert_eq!(
        menu.click(WINDOW_RECT.ulx + 194 + 37, WINDOW_RECT.uly + 294 + 12),
        ResearchAction::Done
    );

    // The right arrow of Attack does nothing at free_capacity = 0.
    let right = PixelRect::new(
        WINDOW_RECT.ulx + RIGHT_ARROW.0,
        WINDOW_RECT.uly + FIRST_ROW_Y,
        ARROW_SIZE.0,
        ARROW_SIZE.1,
    );
    assert_eq!(
        menu.click(right.ulx + 1, right.uly + 1),
        ResearchAction::None
    );
    assert_eq!(menu.values[0], 1);

    // The left arrow frees a center.
    let left = PixelRect::new(
        WINDOW_RECT.ulx + LEFT_ARROW.0,
        WINDOW_RECT.uly + FIRST_ROW_Y,
        ARROW_SIZE.0,
        ARROW_SIZE.1,
    );
    assert_eq!(menu.click(left.ulx + 1, left.uly + 1), ResearchAction::None);
    assert_eq!(menu.values[0], 0);
    assert_eq!(menu.free_capacity, 1);
}

#[test]
fn determinism_of_two_identical_menus() {
    let (a, team) = state_with_center(ResearchTopic::Attack, 0);
    let (b, _) = state_with_center(ResearchTopic::Attack, 0);

    let mut menu_a = ResearchMenu::new(StateRef::from(&a), team).unwrap();
    let mut menu_b = ResearchMenu::new(StateRef::from(&b), team).unwrap();
    for menu in [&mut menu_a, &mut menu_b] {
        menu.adjust(ResearchTopic::Attack as usize, false);
        menu.adjust(ResearchTopic::Hits as usize, true);
        menu.adjust(ResearchTopic::Hits as usize, false);
        menu.adjust(ResearchTopic::Scan as usize, true);
    }
    assert_eq!(menu_a.values, menu_b.values);
    assert_eq!(menu_a.free_capacity, menu_b.free_capacity);
    let (a, actual_a) = commit(&mut menu_a, a);
    let (b, actual_b) = commit(&mut menu_b, b);
    assert_eq!(actual_a, actual_b);
    assert_eq!(a.snapshot().research_allocation(team), b.snapshot().research_allocation(team));
}
