//! Turn Based (PlayMode::TurnBased) — end-to-end local match scenarios.
//!
//! User instruction: all QA and soak — in Turn Based. Here:
//!
//! 1. **First-turn hang regression**: the player fires a rocket and immediately presses
//!    End Turn; the projectile flies during the AI turn. Previously `ROCKET` (speed = 0)
//!    got stuck in `ORDER_MOVE` forever, `OrdersPending` was not reset,
//!    `Ai_CheckEndTurn` was not called — the AI turn hung until the emergency 12 000 frames.
//! 2. **Hot-seat in Turn Based**: two humans + a computer; turn-transition
//!    windows, the AI turn and the return to a human.
//! 3. **SP in Turn Based**: several rounds in a row without hangs, the turn
//!    counter grows.

use std::path::{Path, PathBuf};

use max_app::local::{LocalPhase, LocalSetup};
use max_app::{App, AppConfig};
use max_core::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};
use max_core::enums::{PlayMode, TeamClan, UnitOrder, UnitOrderState, UnitType, VictoryType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn local_app() -> Option<App> {
    let data = repo_path("max-original");
    if !data.join("MAX.RES").is_file() {
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(&data)
        .with_assets_dir(repo_path("assets"))
        .with_mode(max_app::AppMode::Ui);

    let mut app = App::new(config);
    app.init_assets().ok()?;

    Some(app)
}

fn setup_vs_one_ai() -> LocalSetup {
    LocalSetup {
        player_name: "Tester".to_string(),
        world: 5,
        opponent: 2,
        timer: 0,
        endturn: 0,
        play_mode: PlayMode::TurnBased,
        start_gold: 150,
        raw: 1,
        fuel: 1,
        gold: 1,
        derelicts: 0,
        victory_type: VictoryType::Duration,
        victory_limit: 50,
        team_types: [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ],
        team_clans: [TeamClan::Random; 4],
        hot_seat: false,
        seed: 7,
        start_positions: [None; 4],
        team_loadouts: std::array::from_fn(|_| None),
        team_gold: [None; 4],
        team_upgrades: std::array::from_fn(|_| None),
        team_gold_spent: [None; 4],
    }
}

fn run_until_player_turn(app: &mut App, limit: usize) -> bool {
    for _ in 0..limit {
        if app.local().map(|local| local.phase) == Some(LocalPhase::PlayerTurn) {
            return true;
        }
        if app.local().is_none_or(|local| local.finished()) {
            return false;
        }
        app.local_update(0.0);
    }

    app.local().map(|local| local.phase) == Some(LocalPhase::PlayerTurn)
}

fn combat_effect(unit_type: UnitType) -> bool {
    matches!(
        unit_type,
        UnitType::Rocket
            | UnitType::Torpedo
            | UnitType::AlienMissile
            | UnitType::AlienTankBall
            | UnitType::AlienArtilleryBall
            | UnitType::RocketSmoke
            | UnitType::TorpedoBubble
            | UnitType::LandExplosion
            | UnitType::AirExplosion
            | UnitType::SeaExplosion
            | UnitType::BuildingExplosion
            | UnitType::HitExplosion
    )
}

/// First-turn hang: the player's rocket fired before End Turn must not
/// hold the AI turn. Both projectiles and explosions are checked (they do not remain on the map).
#[test]
fn first_turn_end_turn_with_a_missile_in_flight_returns_to_the_player() {
    let Some(mut app) = local_app() else {
        return;
    };

    app.start_local_game(&setup_vs_one_ai())
        .expect("локальная партия Turn Based");
    assert_eq!(
        app.local().map(|local| local.phase),
        Some(LocalPhase::PlayerTurn),
        "первый ход — человек"
    );

    // The target is visible to the shooter (in the match — survey): a deterministic shot.
    app.universe.ai_mut().all_visible = true;
    app.universe.update_visibility_status(false);

    let base = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find_map(|id| {
            app.state_ref()
                .unit(id)
                .filter(|unit| unit.team == TeamId::RED)
                .map(|unit| unit.grid)
        })
        .expect("у RED есть юниты");

    let launcher_grid = Point::new((base.x + 1).min(108), base.y);
    let target_grid = Point::new((base.x + 9).min(110), base.y);

    let launcher = app
        .universe
        .spawn_unit(
            UnitType::MissileCrawler,
            TeamId::RED,
            launcher_grid,
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("снайпер-ракетчик");
    let target = app
        .universe
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            target_grid,
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("цель");
    app.universe.update_visibility_status(false);

    let mut data = app.state_ref().order_data(launcher);
    data.fire_on = target_grid;
    app.universe.set_order_data(launcher, data);
    app.universe
        .set_order(launcher, UnitOrder::Fire, UnitOrderState::Init);

    // The player presses End Turn without waiting for the hit.
    app.local_end_turn();
    assert!(
        matches!(
            app.local().map(|local| local.phase),
            Some(LocalPhase::AiTurn(TeamId::GREEN))
        ),
        "после End Turn ходит компьютер"
    );

    let turn_before = app.state_ref().turn_counter();
    let mut saw_missile = false;
    let mut saw_explosion = false;

    let mut returned = false;
    for _ in 0..3_000 {
        app.local_update(0.0);

        for id in app.state_ref().unit_ids_in_order() {
            if let Some(unit) = app.state_ref().unit(id) {
                if unit.unit_type == UnitType::Rocket {
                    saw_missile = true;
                }
                if combat_effect(unit.unit_type) && unit.unit_type != UnitType::Rocket {
                    saw_explosion = true;
                }
            }
        }

        if app.local().map(|local| local.phase) == Some(LocalPhase::PlayerTurn) {
            returned = true;
            break;
        }
    }

    assert!(
        returned,
        "ход ИИ не вернулся игроку за 3000 кадров (зависание): phase={:?}, \
         counters={:?}",
        app.local().map(|local| local.phase),
        (0..4)
            .map(|team| app.state_ref().ai().players[team].end_turn_retry_counter)
            .collect::<Vec<_>>()
    );
    assert!(
        app.state_ref().turn_counter() > turn_before,
        "раунд должен закрыться (turn_counter)"
    );
    assert!(saw_missile, "снаряд ROCKET должен был появиться");
    assert!(saw_explosion, "взрыв попадания должен был появиться");
    assert!(
        app.state_ref().unit(target).is_none_or(|unit| unit.hits < 24),
        "ракета должна была попасть в цель"
    );

    let leftovers: Vec<_> = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state_ref().unit(id))
        .filter(|unit| combat_effect(unit.unit_type))
        .map(|unit| unit.unit_type)
        .collect();
    assert!(
        leftovers.is_empty(),
        "юниты-эффекты не должны оставаться на карте: {leftovers:?}"
    );
}

/// Hot-seat in Turn Based: two humans + a computer, turn-transition windows.
#[test]
fn hot_seat_turn_based_covers_humans_and_the_computer() {
    let Some(mut app) = local_app() else {
        return;
    };

    let mut setup = setup_vs_one_ai();
    setup.hot_seat = true;
    setup.team_types = [
        TeamKind::Player,
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::None,
    ];
    app.start_local_game(&setup).expect("hot-seat партия");

    // The first human is shown the window by C++ too (`PlayerTeam = ALIEN`).
    assert_eq!(
        app.local()
            .and_then(|local| local.notice.map(|notice| notice.team)),
        Some(TeamId::RED),
        "окно «Begin turn» для RED"
    );
    assert!(app.local_acknowledge_turn());

    for turn in 1..=3 {
        assert_eq!(
            app.local().map(|local| local.phase),
            Some(LocalPhase::PlayerTurn),
            "ход {turn}: ждём человека"
        );

        app.local_end_turn();

        // The second human: the turn-transition window, then their turn.
        assert_eq!(
            app.local()
                .and_then(|local| local.notice.map(|notice| notice.team)),
            Some(TeamId::GREEN),
            "ход {turn}: окно «Begin turn» для GREEN"
        );
        assert!(app.local_acknowledge_turn());
        assert_eq!(
            app.local().map(|local| local.phase),
            Some(LocalPhase::PlayerTurn)
        );

        app.local_end_turn();

        // Computer BLUE: the AI turn and the return to the first human (RED window).
        assert!(
            run_until_player_turn(&mut app, 5_000),
            "ход {turn}: компьютер должен закрыть ход"
        );
        assert_eq!(
            app.local()
                .and_then(|local| local.notice.map(|notice| notice.team)),
            Some(TeamId::RED),
            "ход {turn}: окно «Begin turn» для RED"
        );
        assert!(app.local_acknowledge_turn());
    }

    assert!(app.state_ref().turn_counter() >= 4, "раунды закрываются");
}

/// SP in Turn Based: several rounds in a row, the AI turn closes normally.
#[test]
fn several_turn_based_rounds_close_without_hanging() {
    let Some(mut app) = local_app() else {
        return;
    };

    app.start_local_game(&setup_vs_one_ai()).expect("партия");

    for turn in 1..=4 {
        assert_eq!(app.state_ref().turn_counter(), turn);
        assert_eq!(
            app.local().map(|local| local.phase),
            Some(LocalPhase::PlayerTurn)
        );

        app.local_end_turn();
        assert!(
            run_until_player_turn(&mut app, 5_000),
            "ход {turn}: ИИ должен вернуть ход за 5000 кадров"
        );
    }

    assert_eq!(app.state_ref().turn_counter(), 5);
}
