//! `UnitStats_Menu` (`unitstats.cpp`) — window state, rows and input.

use super::*;
use max_core::enums::{TeamClan, UnitOrder};
use max_core::team::TeamId;
use max_core::universe::{GameUniverse, LocalDispatcher, StateRef};

fn test_state() -> GameUniverse<LocalDispatcher> {
    let mut state = GameUniverse::default();
    *state.team_units_mut(TeamId::RED).expect("RED economy") =
        max_core::team::TeamUnits::with_clan(TeamClan::Random);
    state
}

fn spawn(state: &mut GameUniverse<LocalDispatcher>, unit_type: UnitType) -> UnitId {
    let flags = max_core::access::unit_type_flags(unit_type);
    state
        .spawn_unit(
            unit_type,
            TeamId::RED,
            max_core::geo::Point::new(22, 22),
            flags,
        )
        .expect("unit")
}

#[test]
fn scout_window_has_the_full_statistics_rows_and_description() {
    let mut state = test_state();
    let scout = spawn(&mut state, UnitType::Scout);
    // The statistics window opens for the selected unit, which is not idle.
    if let Some(unit) = state.unit_mut(scout) {
        unit.orders = UnitOrder::Await;
        unit.state = max_core::enums::UnitOrderState::ExecutingOrder;
    }

    let menu = open_for(StateRef::from(&state), scout, None).expect("stats window");

    assert_eq!(menu.unit, scout);
    assert_eq!(menu.unit_type, UnitType::Scout);
    // `UnitStats_DrawStats`: military (attack/shots/range/ammo) + common
    // (armor/hits/scan) + speed + cost rows — the Scout has all of them.
    let labels: Vec<&str> = menu.rows.iter().map(|row| row.label).collect();
    for expected in [
        "Attack", "Shots", "Range", "Ammo", "Armor", "Hits", "Scan", "Speed", "Cost",
    ] {
        assert!(
            labels.contains(&expected),
            "row {expected} missing, got {labels:?}"
        );
    }
    assert!(
        !menu.description.is_empty(),
        "the Scout has a description in SC_UNITS/language.json"
    );
    assert!(menu.portrait.is_none(), "no assets — no portrait");
}

#[test]
fn rows_show_the_units_own_values_against_the_team_base() {
    let mut state = test_state();
    let scout = spawn(&mut state, UnitType::Scout);

    // An upgraded unit: its own `GetBaseValues()` is above the team base, so the
    // statistics row shows the extra pips (current < original).
    if let Some(unit) = state.unit_mut(scout) {
        let mut values = unit.base_values;
        let attack = values.get_attribute(max_core::unit_values::ATTRIB_ATTACK);
        values.set_attribute(max_core::unit_values::ATTRIB_ATTACK, attack + 2);
        unit.base_values = values;
    }

    let menu = open_for(StateRef::from(&state), scout, None).expect("stats window");
    let attack = menu
        .rows
        .iter()
        .find(|row| row.label == mobilize::STAT_ATTACK)
        .expect("attack row");
    let icons = attack.icons.first().expect("attack pips");

    assert!(
        icons.original > icons.current,
        "the unit's own attack is above the team base: current={} original={}",
        icons.current,
        icons.original
    );
}

#[test]
fn input_closes_on_esc_enter_done_and_opens_help() {
    let mut state = test_state();
    let scout = spawn(&mut state, UnitType::Scout);
    let menu = open_for(StateRef::from(&state), scout, None).expect("stats window");

    assert_eq!(
        menu.input(InputEvent::Key(Key::Escape)),
        UnitStatsAction::Close
    );
    assert_eq!(
        menu.input(InputEvent::Key(Key::Enter)),
        UnitStatsAction::Close
    );
    assert_eq!(
        menu.input(InputEvent::Key(Key::Char('?'))),
        UnitStatsAction::Help
    );

    let done = (DONE_RECT.ulx + 2, DONE_RECT.uly + 2);
    assert_eq!(
        menu.input(InputEvent::MouseClick(done.0, done.1)),
        UnitStatsAction::Close
    );
    let help = (HELP_RECT.ulx + 2, HELP_RECT.uly + 2);
    assert_eq!(
        menu.input(InputEvent::MouseClick(help.0, help.1)),
        UnitStatsAction::Help
    );
    // A click inside the window (the portrait) does nothing, and mouse motion too.
    assert_eq!(
        menu.input(InputEvent::MouseClick(PORTRAIT.ulx + 5, PORTRAIT.uly + 5)),
        UnitStatsAction::None
    );
    assert_eq!(
        menu.input(InputEvent::MouseMove(10, 10)),
        UnitStatsAction::None
    );
}

#[test]
fn help_section_maps_to_stats_setup() {
    assert_eq!(crate::helpmenu::HelpSection::Stats.as_str(), "STATS_SETUP");
    assert_eq!(
        crate::helpmenu::HelpSection::parse("stats"),
        Some(crate::helpmenu::HelpSection::Stats)
    );
    assert!(!crate::helpmenu::section_entries(crate::helpmenu::HelpSection::Stats).is_empty());
}
