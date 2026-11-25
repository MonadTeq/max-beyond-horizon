//! Stealth visibility of submerged units and infiltrators.
//!
//! # Oracle
//!
//! * `UnitInfo::InitStealthStatus` (`max-cpp-port/src/unitinfo.cpp:2796`) — mines,
//!   `COMMANDO` and `SUBMARNE` start invisible to every team;
//! * `UnitsManager_IsUnitUnderWater` (`units_manager.cpp:1994`) — a `SUBMARNE` is
//!   always submerged, `CLNTRANS` only on a water cell;
//! * `ResourceManager_InitHeatMaps` (`resource_manager.cpp:1306-1320`) — the
//!   sea-stealth layer of a team's heat map is fed **only by `CORVETTE`**, the
//!   land-stealth layer only by `COMMANDO`/`INFANTRY`;
//! * `HeatMap::Add` (`heatmap.cpp:56-106`) — the stealth layer and its
//!   `on_sea_revealed`/`on_land_revealed` callout fire only when the qualifier
//!   matches **and** the layer was zero;
//! * `Access_UpdateMapStatusAddUnit` (`access.cpp:637-682`) — the callout
//!   accumulates target classes; it does **not** reveal the cell for every
//!   scanning unit;
//! * `Access_DrawUnit` (`access.cpp:691`) — one team's scan spots a submerged
//!   unit only through `GetStealthSea` (`CORVETTE` coverage) and an infiltrator
//!   only through `GetStealthLand`;
//! * `UnitInfo::DrawStealth` (`unitinfo.cpp:2872`) — a stealth unit inside a
//!   scanned cell without its stealth coverage is hidden again (`Draw(team)`),
//!   and `UnitInfo::SpotByTeam` (`:2825-2829`) surfaces a spotted
//!   `SUBMARNE`/`CLNTRANS` (`image_base = 8`).
//!
//! # Defect (fixed)
//!
//! `update_map_status_add_unit` (`max_core/src/ai/threat.rs`) treated a stealth
//! layer that was merely still zero as a **transition**, so
//! `Access_OnSeaStealthRevealed`/`Access_OnLandStealthRevealed` ran for *every*
//! cell any unit scanned and spotted every submerged unit and every infiltrator
//! in it. A submarine was therefore visible to any team whose ordinary scan
//! touched its cell — the corvette was irrelevant. `DrawStealth` also lacked its
//! hide branch, so a submarine that lost the corvette coverage stayed visible for
//! the rest of the game.

use max_core::enums::unit_flags::{MOBILE_LAND_UNIT, MOBILE_SEA_UNIT, SELECTABLE};
use max_core::enums::{GameStateType, PlayMode, TeamClan, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind, TeamUnits};
use max_core::universe::GameUniverse;
use max_core::{UnitId, UnitTypeClassify};

fn scenario() -> GameUniverse {
    let mut state = GameUniverse::default();
    // Real-time (`PLAY_MODE_UNKNOWN`): the turn-based branch of
    // `UnitsManager_ShouldAttack` takes no part in the visibility checks.
    state.set_play_mode(PlayMode::Unknown);
    state.set_game_state(GameStateType::InGame);
    state.teams_mut()[TeamId::RED.0 as usize].team_type = TeamKind::Player;
    state.teams_mut()[TeamId::GREEN.0 as usize].team_type = TeamKind::Player;
    state.team_units_table_mut()[TeamId::RED.0 as usize] = TeamUnits::with_clan(TeamClan::Random);
    state.team_units_table_mut()[TeamId::GREEN.0 as usize] = TeamUnits::with_clan(TeamClan::Random);
    state.set_player_team(TeamId::RED);
    state
}

fn spawn(state: &mut GameUniverse, unit_type: UnitType, team: TeamId, grid: Point) -> UnitId {
    let flags = if unit_type.is_sea() {
        MOBILE_SEA_UNIT
    } else {
        MOBILE_LAND_UNIT
    };
    state
        .spawn_unit(unit_type, team, grid, flags | SELECTABLE)
        .expect("spawn")
}

fn visible(state: &GameUniverse, id: UnitId, team: TeamId) -> bool {
    state
        .snapshot()
        .unit(id)
        .expect("unit")
        .is_visible_to_team(team)
}

/// The corvette dies: with no sea-stealth coverage left the submarine is hidden
/// again (`UnitInfo::DrawStealth` → `Draw(team)`), instead of staying visible for
/// the rest of the game.
///
/// The detection memo is part of the oracle: `UnitInfo::UpdateProduction`
/// (`unitinfo.cpp:2954-2960`) clears `spotted_by_team` for every mobile unit at
/// the turn start, and `DrawStealth`'s first branch (`spotted_by_team`) keeps a
/// detected unit visible until then. The test clears the memo the same way and
/// recomputes the coverage, which is what the next `Access_UpdateMapStatus` pass
/// of the submarine does.
#[test]
fn submarine_dives_again_when_the_corvette_coverage_is_gone() {
    let mut state = scenario();
    let _tank = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(10, 10));
    let sub = spawn(
        &mut state,
        UnitType::Submarine,
        TeamId::GREEN,
        Point::new(12, 10),
    );
    let corvette = spawn(
        &mut state,
        UnitType::Corvette,
        TeamId::RED,
        Point::new(14, 10),
    );

    state.update_visibility_status(false);
    assert!(
        visible(&state, sub, TeamId::RED),
        "the corvette revealed it"
    );

    state.destroy_unit(corvette);
    // `UpdateProduction` of the next turn: `spotted_by_team[i] = false`.
    if let Some(unit) = state.unit_mut(sub) {
        unit.spotted_by_team = [false; 5];
    }
    state.update_visibility_status(false);

    assert_eq!(
        state
            .snapshot()
            .ai_heat_map_stealth_sea(TeamId::RED, 12, 10),
        0,
        "the coverage is gone with the corvette"
    );
    assert!(
        !visible(&state, sub, TeamId::RED),
        "without its stealth coverage the submarine is hidden again"
    );
}

/// The same root cause on the land layer: `INFANTRY`/`COMMANDO` are the only
/// land-stealth sources, an ordinary scan must not reveal an infiltrator.
#[test]
fn infiltrator_is_hidden_from_an_ordinary_scan() {
    let mut state = scenario();
    let _tank = spawn(&mut state, UnitType::Tank, TeamId::RED, Point::new(10, 10));
    let commando = spawn(
        &mut state,
        UnitType::Infiltrator,
        TeamId::GREEN,
        Point::new(12, 10),
    );

    state.update_visibility_status(false);

    assert!(
        state.snapshot().ai_heat_map_complete(TeamId::RED, 12, 10) > 0,
        "the ordinary scan covers the infiltrator's cell"
    );
    assert_eq!(
        state
            .snapshot()
            .ai_heat_map_stealth_land(TeamId::RED, 12, 10),
        0,
        "no infantry/commando covers the cell"
    );
    assert!(
        !visible(&state, commando, TeamId::RED),
        "an infiltrator must stay hidden from an ordinary scan"
    );
}

/// An `INFANTRY` (the land-stealth qualifier) reveals the infiltrator.
#[test]
fn infantry_land_stealth_coverage_reveals_the_infiltrator() {
    let mut state = scenario();
    let commando = spawn(
        &mut state,
        UnitType::Infiltrator,
        TeamId::GREEN,
        Point::new(12, 10),
    );

    state.update_visibility_status(false);
    assert!(!visible(&state, commando, TeamId::RED));

    let _infantry = spawn(
        &mut state,
        UnitType::Infantry,
        TeamId::RED,
        Point::new(13, 10),
    );
    state.update_visibility_status(false);

    assert!(
        state
            .snapshot()
            .ai_heat_map_stealth_land(TeamId::RED, 12, 10)
            > 0,
        "the infantry feeds the land-stealth layer"
    );
    assert!(
        visible(&state, commando, TeamId::RED),
        "infantry land-stealth coverage reveals the infiltrator"
    );
}

/// `UnitsManager_IsUnitUnderWater`: a `CLNTRANS` (`ArmouredPersonnelCarrier`) is
/// submerged only while it stands on water, while a `SUBMARNE` always is. The
/// observable consequence is visibility: the carrier on a water cell inside an
/// ordinary scan stays hidden, on a land cell it is spotted.
#[test]
fn carrier_is_hidden_only_on_water() {
    let mut state = scenario();
    let tank_cell = Point::new(10, 10);
    let water_cell = Point::new(11, 10);
    let land_cell = Point::new(12, 10);

    // The generated map is not guaranteed to hold a coast next to a land cell, so
    // the three cells of the scenario are set explicitly.
    state
        .map_mut()
        .surface
        .set(water_cell.x, water_cell.y, max_core::Surface::WATER);
    state
        .map_mut()
        .surface
        .set(land_cell.x, land_cell.y, max_core::Surface::LAND);

    let _tank = spawn(&mut state, UnitType::Tank, TeamId::RED, tank_cell);
    let on_water = spawn(
        &mut state,
        UnitType::ArmouredPersonnelCarrier,
        TeamId::GREEN,
        water_cell,
    );
    let on_land = spawn(
        &mut state,
        UnitType::ArmouredPersonnelCarrier,
        TeamId::GREEN,
        land_cell,
    );

    state.update_visibility_status(false);

    assert!(
        !visible(&state, on_water, TeamId::RED),
        "the carrier on water is submerged and stays hidden from an ordinary scan"
    );
    assert!(
        visible(&state, on_land, TeamId::RED),
        "the carrier on land is not submerged and is spotted by the scan"
    );
}
