use super::*;
use crate::enums::Surface;
use crate::input::{MissionSupplyEntry, Packet12MissionSupply};
use crate::team::TeamUnits;

/// A map with the given land rectangles `(x0, y0, x1, y1)`; ends are exclusive.
fn land_state(rects: &[(i16, i16, i16, i16)]) -> GameState {
    let mut state = GameState::new();

    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            state.map.surface.set(x, y, Surface::WATER);
        }
    }

    for &(x0, y0, x1, y1) in rects {
        for y in y0..y1 {
            for x in x0..x1 {
                state.map.surface.set(x, y, Surface::LAND);
            }
        }
    }

    state
}

/// `ResourceManager_InitClanUnitValues` starts from a clean `SC_ATTRI` base:
/// a second call (or a clan change) must replace the table, not add to it,
/// and the team complexes are dropped (`ClearComplexes`).
#[test]
fn apply_clan_reinitializes_instead_of_stacking() {
    let mut world = GameState::new();
    let team = TeamId::RED;
    let index = team.0 as usize;
    world.team_units[index] = TeamUnits::with_clan(TeamClan::Musashi);
    world.team_units[index].create_complex();

    apply_clan(&mut world, team, TeamClan::VonGriffin);
    let expected = crate::unit_values::team_base_values(TeamClan::VonGriffin);
    assert_eq!(world.team_units[index].base_values, expected);
    assert_eq!(world.team_units[index].current_values, expected);
    assert!(world.team_units[index].complexes().is_empty());

    // Idempotent: the same clan applied again keeps the same table.
    apply_clan(&mut world, team, TeamClan::VonGriffin);
    assert_eq!(world.team_units[index].base_values, expected);
    assert_eq!(world.team_units[index].current_values, expected);
}

/// `ContinentFiller` builds `MAXFloodFill` with diagonal expansion: runs of
/// adjacent columns connect when their intervals touch diagonally, so
/// diagonally adjacent land is one continent (not two 4-connected ones).
#[test]
fn land_continents_connect_diagonally() {
    let state = land_state(&[(10, 10, 14, 14), (14, 14, 18, 18)]);
    let continents = land_continents(&state);

    assert_eq!(
        continents.list.len(),
        1,
            "diagonally touching land must be a single continent"
    );
    assert_eq!(continents.list[0].size, 32);
    assert_eq!(continents.list[0].bounds, (10, 10, 18, 18));
    assert_eq!(continents.list[0].filler, 3);
}

/// `IsViableSite` needs a flat 4×4 area of one continent; a three-column strip
/// is rejected and the continent's filler is cleared (`~Continent`).
#[test]
fn viable_continent_requires_a_flat_four_by_four_site() {
    let narrow = land_continents(&land_state(&[(10, 10, 13, 20)]));
    assert!(narrow.list.is_empty(), "a 3×10 strip has no 4×4 site");
    assert_eq!(filler_at(&narrow.cells, Point::new(10, 10)), 0);

    let state = land_state(&[(10, 10, 14, 20)]);
    let wide = land_continents(&state);
    assert_eq!(wide.list.len(), 1);
    assert_eq!(wide.list[0].bounds, (10, 10, 14, 20));

    let filler = wide.list[0].filler;
    assert!(is_flat_site(&wide.cells, filler, Point::new(10, 10)));
    assert!(!is_flat_site(&wide.cells, filler, Point::new(11, 10)));
    assert!(has_viable_site(&wide.cells, filler, wide.list[0].bounds));
}

/// `TestIsolated`: only close continents are tested, and the flood reaches
/// another land mass when it lies inside the `bounds + 6` zone.
#[test]
fn test_isolated_flags_land_within_the_zone_only() {
    let proximity_range = 14;

    // Two 10×10 islands two water cells apart: the second is inside the zone
    // of the first, so the first is not isolated.
    let mut near = land_continents(&land_state(&[(10, 10, 20, 20), (22, 10, 32, 20)]));
    assert_eq!(near.list.len(), 2);
    near.test_isolated(0, proximity_range);
    assert!(!near.list[0].is_isolated);

    // The same island alone has no other land mass in the zone.
    let mut alone = land_continents(&land_state(&[(10, 10, 20, 20)]));
    alone.test_isolated(0, proximity_range);
    assert!(alone.list[0].is_isolated);

    // Wider than `2 × proximity_range`: `IsCloseProximity` is false and the
    // test does not run, so the flag keeps its constructor value.
    let mut wide = land_continents(&land_state(&[(10, 10, 60, 60)]));
    wide.test_isolated(0, proximity_range);
    assert!(!wide.list[0].is_isolated);
}

/// `IsViableSite(test_proximity = true)` rejects a site within
/// `proximity_range + 4` of another team's landing point (`IsDangerousProximity`).
#[test]
fn viable_site_rejects_enemy_landing_proximity() {
    let mut state = land_state(&[(10, 10, 60, 60)]);
    state.mission_supplies[TeamId::GREEN.0 as usize] = Some(Packet12MissionSupply {
        team: TeamId::GREEN,
        round: 0,
        start_gold: 0,
        team_gold: 0,
        units: vec![MissionSupplyEntry {
            unit_type: UnitType::Constructor,
            cargo: 0,
        }],
        gold_spent_on_upgrades: 0,
        starting_position: Point::new(20, 20),
        proximity_alert_ack: 0,
    });

    let continents = land_continents(&state);
    assert_eq!(continents.list.len(), 1);
    let filler = continents.list[0].filler;

    assert!(!continents.is_viable_site(
        &state,
        filler,
        true,
        TeamId::RED,
        Point::new(20, 20),
        14
    ));
    assert!(continents.is_viable_site(
        &state,
        filler,
        true,
        TeamId::RED,
        Point::new(40, 40),
        14
    ));
    // Without the proximity test the flat-site criterion still accepts the site.
    assert!(continents.is_viable_site(
        &state,
        filler,
        false,
        TeamId::RED,
        Point::new(20, 20),
        14
    ));
}

/// A recognized `menu_team_strategy_setting` with a non-zero score forces the
/// strategy; the landing point is a flat 4×4 site and is written to both
/// `target_location` and `TeamMissionSupplies.starting_position`.
#[test]
fn select_strategy_honors_forced_strategy_and_selects_a_landing_site() {
    let mut state = land_state(&[(10, 10, 30, 30)]);
    state.ai.settings.team_strategy[TeamId::RED.0 as usize] = Some(AiStrategy::Sea);
    state.mission_supplies[TeamId::RED.0 as usize] = Some(Packet12MissionSupply {
        team: TeamId::RED,
        round: 0,
        start_gold: 0,
        team_gold: 0,
        units: Vec::new(),
        gold_spent_on_upgrades: 0,
        starting_position: Point::ZERO,
        proximity_alert_ack: 0,
    });

    assert!(crate::ai::ai_setup_strategy(&mut state, TeamId::RED));
    assert_eq!(
        state.ai.players[TeamId::RED.0 as usize].strategy,
        AiStrategy::Sea
    );

    let landing = state.ai.players[TeamId::RED.0 as usize].target_location;
    assert_eq!(
        state.mission_supplies[TeamId::RED.0 as usize]
            .as_ref()
            .expect("supplies")
            .starting_position,
        landing
    );

    for dy in 0..4 {
        for dx in 0..4 {
            assert_eq!(
                state.map.surface_at(landing.x + dx, landing.y + dy),
                Surface::LAND,
                    "the landing site must be a flat 4×4 land area"
            );
        }
    }
}

/// A forced strategy whose score is zero does not force anything: C++ falls
/// back to the score-weighted roll (`aiplayer.cpp:3896-3897`).
#[test]
fn forced_strategy_with_zero_score_falls_back_to_the_roll() {
    // The default map is one huge continent: not close, therefore not isolated
    // and `AI_STRATEGY_SEA` scores zero.
    let mut state = GameState::new();
    state.ai.settings.team_strategy[TeamId::RED.0 as usize] = Some(AiStrategy::Sea);

    assert!(crate::ai::ai_setup_strategy(&mut state, TeamId::RED));

    let strategy = state.ai.players[TeamId::RED.0 as usize].strategy;
    assert_ne!(strategy, AiStrategy::Sea);
    assert_ne!(strategy, AiStrategy::Random);
}

/// `menu_team_strategy_setting` values are matched case-insensitively;
/// `random` and unknown values leave the strategy to the roll.
#[test]
fn team_strategy_setting_names_are_case_insensitive() {
    assert_eq!(
        parse_team_strategy("combined arms"),
        Some(AiStrategy::CombinedArms)
    );
    assert_eq!(
        parse_team_strategy("FAST ATTACK"),
        Some(AiStrategy::FastAttack)
    );
    assert_eq!(parse_team_strategy("random"), None);
    assert_eq!(parse_team_strategy(""), None);
    assert_eq!(parse_team_strategy("tank"), None);
}

/// `AiPlayer::BeginTurn` → `DetermineAttack` creates a `TaskAttack` for a
/// spotted key facility. The player is temporarily removed from
/// `world.ai.players` during `BeginTurn`, so the spotted units must come
/// from `self`.
#[test]
fn begin_turn_creates_an_attack_for_a_spotted_key_facility() {
    let mut state = land_state(&[(0, 0, 24, 24)]);
    state.teams[TeamId::RED.0 as usize].team_type = TeamKind::Computer;
    state.teams[TeamId::RED.0 as usize].team_clan = TeamClan::Musashi;
    state.teams[TeamId::GREEN.0 as usize].team_type = TeamKind::Player;
    state.team_units[TeamId::RED.0 as usize] = TeamUnits::with_clan(TeamClan::Musashi);
    state.team_units[TeamId::GREEN.0 as usize] = TeamUnits::with_clan(TeamClan::Musashi);
    state.ai.settings.opponent = crate::enums::Opponent::Average as u8;
    crate::ai::ai_init(&mut state);

    state
        .spawn_unit(
            crate::enums::UnitType::PowerStation,
            TeamId::RED,
            Point::new(12, 10),
            0,
        )
        .expect("red building");
    state
        .spawn_unit(
            crate::enums::UnitType::Tank,
            TeamId::RED,
            Point::new(5, 5),
            0,
        )
        .expect("red tank");

    let enemy = state
        .spawn_unit(
            crate::enums::UnitType::PowerStation,
            TeamId::GREEN,
            Point::new(14, 10),
            crate::enums::unit_flags::SELECTABLE,
        )
        .expect("green building");
    let mut info = state.unit(enemy).expect("green building").clone();
    info.set_visible_to_team(TeamId::RED, true);
    state.write_unit(enemy, info);

    crate::ai::ai_unit_spotted(&mut state, enemy, TeamId::RED);
    assert!(crate::ai::ai_setup_strategy(&mut state, TeamId::RED));

    crate::ai::ai_begin_turn(&mut state, TeamId::RED);

    assert!(
        state
            .ai
            .task_manager
            .tasks()
            .iter()
            .any(|task| matches!(task.kind, TaskKind::Attack(_))),
            "DetermineAttack must create a TaskAttack for a spotted key facility"
    );
}
