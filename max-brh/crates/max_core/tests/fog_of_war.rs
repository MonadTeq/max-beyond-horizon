//! Fog of war: visibility of foreign units (`UnitInfo::IsVisibleToTeam`).
//!
//! Oracle — `unitinfo.cpp:2796` (`InitStealthStatus`), `unitinfo.cpp:2810`
//! (`SpotByTeam`), `access.cpp:762` (`Access_UpdateMapStatus`) and
//! `access.cpp:880` (`Access_UpdateVisibilityStatus`). Before the fix `spawn_unit`
//! marked ordinary units visible to **all** teams, so foreign bases were visible
//! on the map and minimap without reconnaissance.

use max_core::enums::{TeamClan, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_core::universe::GameUniverse;

fn state() -> GameUniverse {
    let mut state = GameUniverse::default();
    state.teams_mut()[TeamId::RED.0 as usize].team_type = TeamKind::Player;
    state.teams_mut()[TeamId::GREEN.0 as usize].team_type = TeamKind::Player;
    state.team_units_table_mut()[TeamId::RED.0 as usize] =
        max_core::team::TeamUnits::with_clan(TeamClan::Random);
    state.team_units_table_mut()[TeamId::GREEN.0 as usize] =
        max_core::team::TeamUnits::with_clan(TeamClan::Random);
    state.set_player_team(TeamId::RED);
    state
}

#[test]
fn spawned_units_are_visible_only_to_their_own_team() {
    let mut state = state();
    let own = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(20, 20), 0)
        .expect("свой танк");
    let enemy = state
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(80, 80), 0)
        .expect("чужой танк");

    assert!(state
        .snapshot()
        .unit(own)
        .unwrap()
        .is_visible_to_team(TeamId::RED));
    assert!(!state
        .snapshot()
        .unit(own)
        .unwrap()
        .is_visible_to_team(TeamId::GREEN));
    assert!(!state
        .snapshot()
        .unit(enemy)
        .unwrap()
        .is_visible_to_team(TeamId::RED));
    assert!(state
        .snapshot()
        .unit(enemy)
        .unwrap()
        .is_visible_to_team(TeamId::GREEN));
}

#[test]
fn scan_reveals_enemies_in_fov_but_not_outside_it() {
    let mut state = state();
    let own = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(20, 20), 0)
        .expect("свой танк");
    let near = state
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(21, 20), 0)
        .expect("ближний враг");
    let far = state
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(80, 80), 0)
        .expect("дальний враг");

    // `UnitsManager_DeployUnit` в†’ `Access_UpdateMapStatus(unit, true)` в†’
    // `Access_DrawUnit` (`access.cpp:762,691`): a unit deployed inside a cell
    // the team has already scanned is spotted at once, without a separate
    // `Access_UpdateVisibilityStatus` pass.
    assert!(
        state
            .snapshot()
            .unit(near)
            .unwrap()
            .is_visible_to_team(TeamId::RED),
        "deployment inside the scanned area spots the enemy immediately"
    );

    // `Access_UpdateVisibilityStatus(false)`: rebuilds scan coverage.
    state.update_visibility_status(false);

    assert!(
        state
            .snapshot()
            .unit(near)
            .unwrap()
            .is_visible_to_team(TeamId::RED),
        "враг в радиусе обзора (TANK scan = 4) обнаружен"
    );
    assert!(
        state
            .snapshot()
            .unit(near)
            .unwrap()
            .is_detected_by_team(TeamId::RED),
        "SpotByTeam ставит spotted_by_team"
    );
    assert!(
        !state
            .snapshot()
            .unit(far)
            .unwrap()
            .is_visible_to_team(TeamId::RED),
        "враг вне FOV остаётся скрытым"
    );
    assert!(
        state.snapshot().ai_heat_map_complete(TeamId::RED, 20, 20) > 0,
        "клетка своего юнита разведана"
    );
    assert_eq!(
        state.snapshot().ai_heat_map_complete(TeamId::RED, 80, 80),
        0,
        "дальняя клетка не разведана"
    );

    let _ = own;
}

#[test]
fn all_visible_marks_every_spawned_unit_visible() {
    let mut state = state();
    state.ai_mut().all_visible = true;

    let own = state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(20, 20), 0)
        .unwrap();
    let enemy = state
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(80, 80), 0)
        .unwrap();

    // `InitStealthStatus`: invisible types — no, ordinary ones — `AllVisible`.
    assert!(state
        .snapshot()
        .unit(enemy)
        .unwrap()
        .is_visible_to_team(TeamId::RED));
    assert!(state
        .snapshot()
        .unit(own)
        .unwrap()
        .is_visible_to_team(TeamId::GREEN));

    state.update_visibility_status(true);
    assert!(state
        .snapshot()
        .unit(enemy)
        .unwrap()
        .is_visible_to_team(TeamId::RED));
}

#[test]
fn stealth_units_stay_hidden_outside_fov() {
    let mut state = state();
    state
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(20, 20), 0)
        .unwrap();
    // `COMMANDO` outside the tank's scan radius (scan 4) — not visible.
    let commando = state
        .spawn_unit(UnitType::Infiltrator, TeamId::GREEN, Point::new(40, 40), 0)
        .unwrap();

    state.update_visibility_status(false);

    assert!(
        !state
            .snapshot()
            .unit(commando)
            .unwrap()
            .is_visible_to_team(TeamId::RED),
        "COMMANDO вне FOV не обнаруживается"
    );

    // Explicit detection (e.g. a failed stealth action) makes it visible.
    state
        .unit_mut(commando)
        .unwrap()
        .set_visible_to_team(TeamId::RED, true);
    assert!(state
        .snapshot()
        .unit(commando)
        .unwrap()
        .is_visible_to_team(TeamId::RED));
}
