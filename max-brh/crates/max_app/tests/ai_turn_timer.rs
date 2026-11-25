//! Real-time AI turn: the `settings["endturn"]` timer closes the turn.
//!
//! Owner decision 2026-11-25: in real time the AI turn ends by the `endturn`
//! timer, 1:1 with the oracle, instead of the port-only
//! `end_turn_retry_counter > 120` shortcut. The counter stays as the fallback
//! when the setting is absent (`End Turn: None`) and as the Turn Based trigger,
//! where the oracle itself closes the AI turn.
//!
//! Oracle sources:
//! * `GameManager_HandleTurnTimer` (`max-cpp-port/src/game_manager.cpp:893-916`) —
//!   real time: `settings["endturn"]`, floored to 15 s while a computer team has
//!   not finished its turn;
//! * `GameManager_UpdateGui` (`game_manager.cpp:2470-2474`) — Turn Based:
//!   `settings["endturn"]`, floored to 45 s (`00:45` in the panel);
//! * `AiPlayer::CheckEndTurn` (`aiplayer.cpp:2828-2848`) — `RefreshOrders` at
//!   `> 120` only for `PLAY_MODE_TURN_BASED`/`GAME_STATE_9_END_TURN`/demo, so in
//!   real time the AI never closes its own turn;
//! * `settings.cpp:118` — the `endturn` default is 45.
//!
//! Wall clock: the simulation runs fixed 60 Hz steps (`presentation::SIMULATION_HZ`),
//! so a window of `endturn` seconds is exactly `endturn × 60` ticks — the interval
//! the oracle's timer covers. QA/headless advance one tick per frame, so the same
//! count holds there.
//!
//! # Shots/ammo gate (condition 4)
//!
//! The reaction scenario below is the `combat-tb` engagement: a RED rocket
//! crawler fires at an adjacent GREEN tank; during the AI turn the tank spends its
//! per-turn shot budget on the crawler. `UnitInfo::PrepareFire`
//! (`unitinfo.cpp:4709-4820`) spends exactly one ammo and one shot per shot, and
//! `UnitInfo::RecomputeUnitSpeed` resets `shots` to `rounds × speed / speed_attr`
//! at the turn start — `rounds = 2` for `TANK` (`SC_UNITS`), so the oracle fires
//! **2 shots / 2 ammo** in a real-time turn regardless of how long the window is
//! (a longer window must not create extra shots). The gate asserts exactly that
//! for a 15 s and a 45 s window: the same shots, on the same ticks, the same ammo
//! spent, and the AI still acting after tick 121 (the old counter window) — i.e.
//! the interval comes from the timer and the per-interval expenditure is 1:1.

use std::path::{Path, PathBuf};

use max_app::local::{LocalPhase, LocalSetup, ENDTURN_MIN_SECONDS, ENDTURN_TURN_BASED_MIN_SECONDS};
use max_app::preferences::Preferences;
use max_app::presentation::SIMULATION_HZ;
use max_app::{App, AppConfig};
use max_core::enums::unit_flags::{MOBILE_LAND_UNIT, SELECTABLE};
use max_core::enums::{PlayMode, TeamClan, UnitType, VictoryType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};

const TICKS_PER_SECOND: usize = SIMULATION_HZ as usize;

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

fn setup_vs_one_ai(endturn: u32, play_mode: PlayMode) -> LocalSetup {
    LocalSetup {
        player_name: "Tester".to_string(),
        world: 5,
        opponent: 2,
        timer: 0,
        endturn,
        play_mode,
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

/// Ticks of the AI turn of `team`, until the phase leaves `AiTurn`.
fn run_ai_turn(app: &mut App, team: TeamId, limit: usize) -> usize {
    let mut ticks = 0;

    while ticks < limit {
        if !matches!(app.local().map(|local| local.phase), Some(LocalPhase::AiTurn(t)) if t == team)
        {
            break;
        }
        app.local_update(1.0 / SIMULATION_HZ as f32);
        ticks += 1;
    }

    ticks
}

/// `combat-tb` engagement: a RED rocket crawler fires at an adjacent GREEN tank.
///
/// Returns the crawler and the tank. During the AI turn the tank spots the
/// crawler (the shot reveals it), reacts and spends its shot budget on it.
fn reaction_scenario(app: &mut App) -> (max_core::UnitId, max_core::UnitId) {
    app.universe_mut().ai_mut().all_visible = true;
    app.universe_mut().update_visibility_status(false);

    let red_base = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .find_map(|id| {
            app.state_ref()
                .unit(id)
                .filter(|unit| unit.team == TeamId::RED)
                .map(|unit| unit.grid)
        })
        .expect("RED units");

    let target_grid = Point::new((red_base.x + 3).min(110), red_base.y);
    let crawler = app
        .universe_mut()
        .spawn_unit(
            UnitType::MissileCrawler,
            TeamId::RED,
            red_base,
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("crawler");
    let tank = app
        .universe_mut()
        .spawn_unit(
            UnitType::Tank,
            TeamId::GREEN,
            target_grid,
            MOBILE_LAND_UNIT | SELECTABLE,
        )
        .expect("tank");
    app.universe_mut().update_visibility_status(false);

    let mut data = app.state_ref().order_data(crawler);
    data.fire_on = target_grid;
    app.universe_mut().set_order_data(crawler, data);
    app.universe_mut().set_order(
        crawler,
        max_core::enums::UnitOrder::Fire,
        max_core::enums::UnitOrderState::Init,
    );

    (crawler, tank)
}

/// Shots of the GREEN team during its turn: `(tick, unit id, ammo spent)`.
///
/// Ammo decreases by one per shot (`UnitInfo::PrepareFire`); an increase is a
/// reload (`ORDER_RELOAD`, the supply truck refills the magazine), counted
/// separately as `(tick, unit id, ammo restored)`.
#[derive(Debug, Default, PartialEq, Eq)]
struct AiFire {
    ticks: usize,
    shots: Vec<(usize, u16, u8)>,
    reloads: Vec<(usize, u16, u8)>,
}

fn run_ai_turn_with_fire(app: &mut App, team: TeamId, limit: usize) -> AiFire {
    let mut report = AiFire::default();

    let mut ammo: Vec<(u16, u8)> = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| {
            app.state_ref()
                .unit(id)
                .filter(|unit| unit.team == team)
                .map(|unit| (unit.id.0, unit.ammo))
        })
        .collect();
    ammo.sort();

    while report.ticks < limit {
        if !matches!(app.local().map(|local| local.phase), Some(LocalPhase::AiTurn(t)) if t == team)
        {
            break;
        }
        app.local_update(1.0 / SIMULATION_HZ as f32);
        report.ticks += 1;

        for (id, before) in ammo.iter_mut() {
            let Some(unit) = app.state_ref().unit(max_core::UnitId(*id)) else {
                continue;
            };

            if *before > unit.ammo {
                report
                    .shots
                    .push((report.ticks, *id, before.saturating_sub(unit.ammo)));
            } else if *before < unit.ammo {
                report
                    .reloads
                    .push((report.ticks, *id, unit.ammo.saturating_sub(*before)));
            }
            *before = unit.ammo;
        }
    }

    report
}

/// Real time, `endturn = 45` (the default): the AI turn lasts exactly 45 s.
#[test]
fn real_time_ai_turn_runs_for_the_endturn_setting() {
    let Some(mut app) = local_app() else {
        return;
    };

    app.start_local_game(&setup_vs_one_ai(45, PlayMode::SimultaneousMoves))
        .expect("real-time local match");
    app.local_end_turn();
    assert_eq!(
        app.local().map(|local| local.phase),
        Some(LocalPhase::AiTurn(TeamId::GREEN))
    );
    assert_eq!(
        app.local().map(|local| local.timer_left),
        Some(45.0),
        "the panel starts from settings[\"endturn\"] (00:45)"
    );

    let ticks = run_ai_turn(&mut app, TeamId::GREEN, 45 * TICKS_PER_SECOND + 10);

    assert_eq!(ticks, 45 * TICKS_PER_SECOND, "45 s window = 2700 ticks");
    assert_eq!(
        app.local().map(|local| local.phase),
        Some(LocalPhase::PlayerTurn),
        "the timer closes the AI turn"
    );
    assert_eq!(app.local().map(|local| local.timer_left), Some(0.0));
}

/// `GameManager_HandleTurnTimer`: below 15 the window is raised to 15 s.
#[test]
fn endturn_minimum_is_fifteen_seconds() {
    let Some(mut app) = local_app() else {
        return;
    };

    // A value below the oracle floor (the dialog offers 15, this is the clamp).
    app.start_local_game(&setup_vs_one_ai(5, PlayMode::SimultaneousMoves))
        .expect("real-time local match");
    app.local_end_turn();
    assert_eq!(
        app.local().map(|local| local.timer_left),
        Some(ENDTURN_MIN_SECONDS as f32),
        "min 15 s (`GameManager_HandleTurnTimer`)"
    );

    let ticks = run_ai_turn(
        &mut app,
        TeamId::GREEN,
        ENDTURN_MIN_SECONDS as usize * TICKS_PER_SECOND + 10,
    );

    assert_eq!(
        ticks,
        ENDTURN_MIN_SECONDS as usize * TICKS_PER_SECOND,
        "15 s window = 900 ticks"
    );
}

/// The live settings are the oracle source: the next AI turn reads the new value.
#[test]
fn the_next_ai_turn_reads_the_live_preferences() {
    let Some(mut app) = local_app() else {
        return;
    };

    app.start_local_game(&setup_vs_one_ai(15, PlayMode::SimultaneousMoves))
        .expect("real-time local match");

    // The `Preferences` dialog writes the settings object; the next
    // `GameManager_UpdateGui`/`HandleTurnTimer` reads it.
    let mut preferences: Preferences = app.preferences().clone();
    preferences.endturn = 45;
    app.set_preferences(preferences);

    app.local_end_turn();
    assert_eq!(
        app.local().map(|local| local.timer_left),
        Some(45.0),
        "the AI window follows the live settings, not the setup snapshot"
    );

    let ticks = run_ai_turn(&mut app, TeamId::GREEN, 45 * TICKS_PER_SECOND + 10);
    assert_eq!(ticks, 45 * TICKS_PER_SECOND);
}

/// Condition 4 gate: the same per-turn shots/ammo in the 15 s and the 45 s
/// window, and the AI keeps acting after the old 121-tick counter window.
#[test]
fn endturn_15_and_45_spend_the_same_shots_and_ammo() {
    let Some(mut short) = local_app() else {
        return;
    };
    let Some(mut long) = local_app() else {
        return;
    };

    short
        .start_local_game(&setup_vs_one_ai(15, PlayMode::SimultaneousMoves))
        .expect("15 s match");
    let (short_crawler, short_tank) = reaction_scenario(&mut short);
    short.local_end_turn();
    let short_fire = run_ai_turn_with_fire(&mut short, TeamId::GREEN, 15 * TICKS_PER_SECOND + 10);

    long.start_local_game(&setup_vs_one_ai(45, PlayMode::SimultaneousMoves))
        .expect("45 s match");
    let (long_crawler, long_tank) = reaction_scenario(&mut long);
    long.local_end_turn();
    let mut activity_at_121 = None;
    let mut long_fire = AiFire::default();
    {
        // Tick loop with an extra probe at tick 121 (the old counter window).
        let mut ammo: Vec<(u16, u8)> = long
            .state_ref()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| {
                long.state_ref()
                    .unit(id)
                    .filter(|unit| unit.team == TeamId::GREEN)
                    .map(|unit| (unit.id.0, unit.ammo))
            })
            .collect();
        ammo.sort();

        while long_fire.ticks < 45 * TICKS_PER_SECOND + 10
            && matches!(
                long.local().map(|local| local.phase),
                Some(LocalPhase::AiTurn(TeamId::GREEN))
            )
        {
            long.local_update(1.0 / SIMULATION_HZ as f32);
            long_fire.ticks += 1;

            for (id, before) in ammo.iter_mut() {
                let Some(unit) = long.state_ref().unit(max_core::UnitId(*id)) else {
                    continue;
                };
                if *before > unit.ammo {
                    long_fire
                        .shots
                        .push((long_fire.ticks, *id, before.saturating_sub(unit.ammo)));
                } else if *before < unit.ammo {
                    long_fire.reloads.push((
                        long_fire.ticks,
                        *id,
                        unit.ammo.saturating_sub(*before),
                    ));
                }
                *before = unit.ammo;
            }

            if long_fire.ticks == 121 {
                activity_at_121 = Some(ai_activity(&long));
            }
        }
    }

    // The window: `settings["endturn"]` seconds, not the counter.
    assert_eq!(short_fire.ticks, 15 * TICKS_PER_SECOND);
    assert_eq!(long_fire.ticks, 45 * TICKS_PER_SECOND);

    // Oracle per-turn budget: `TANK` rounds = 2 → 2 shots, 2 ammo, no reload
    // (`UnitInfo::PrepareFire` spends one ammo and one shot per shot).
    assert_eq!(
        short_fire.shots.len(),
        2,
        "oracle shot budget in 15 s: {short_fire:?}"
    );
    assert_eq!(
        long_fire.shots.len(),
        2,
        "oracle shot budget in 45 s: {long_fire:?}"
    );
    assert_eq!(
        short_fire.shots, long_fire.shots,
        "same interval → same shots on the same ticks"
    );
    assert_eq!(
        short_fire.reloads, long_fire.reloads,
        "same interval → same reloads"
    );
    assert!(
        short_fire.shots.iter().all(|(_, _, spent)| *spent == 1),
        "one ammo per shot"
    );

    let short_spent: u32 = short_fire.shots.iter().map(|(_, _, n)| u32::from(*n)).sum();
    let long_spent: u32 = long_fire.shots.iter().map(|(_, _, n)| u32::from(*n)).sum();
    assert_eq!(short_spent, 2, "ammo spent in 15 s");
    assert_eq!(long_spent, 2, "ammo spent in 45 s");

    // Both shots happen before tick 121 here; the *window* is what the old
    // counter trigger got wrong. The AI must still be acting at the end of the
    // long window, because the turn is the full 45 s (`Ai_CheckEndTurn` keeps
    // running its economy/attack tasks).
    assert!(
        activity_at_121.is_some_and(|at| at != ai_activity(&long)),
        "the AI must keep acting after tick 121 in the 45 s window"
    );

    // The engagement resolved the same way in both windows.
    assert_eq!(
        short.state_ref().unit(short_crawler).map(|u| u.hits),
        None,
        "the AI tank destroys the crawler inside 15 s"
    );
    assert_eq!(
        long.state_ref().unit(long_crawler).map(|u| u.hits),
        None,
        "the AI tank destroys the crawler inside 45 s"
    );
    assert_eq!(
        short.state_ref().unit(short_tank).map(|u| u.ammo),
        long.state_ref().unit(long_tank).map(|u| u.ammo),
        "the same ammo left on the AI tank"
    );
}

/// GREEN team activity: unit cells/orders plus the task count.
fn ai_activity(app: &App) -> (Vec<(u16, i16, i16, u8)>, usize) {
    let mut units: Vec<(u16, i16, i16, u8)> = app
        .state_ref()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| {
            app.state_ref()
                .unit(id)
                .filter(|unit| unit.team == TeamId::GREEN)
                .map(|unit| (unit.id.0, unit.grid.x, unit.grid.y, unit.orders as u8))
        })
        .collect();
    units.sort();
    (units, app.state_ref().ai().task_manager.task_count())
}

/// Turn Based keeps the oracle trigger: the AI closes its own turn at
/// `end_turn_retry_counter > 120` (`aiplayer.cpp:2838-2848`), timer `max(endturn, 45)`.
#[test]
fn turn_based_keeps_the_oracle_counter_trigger() {
    let Some(mut app) = local_app() else {
        return;
    };

    app.start_local_game(&setup_vs_one_ai(45, PlayMode::TurnBased))
        .expect("turn based match");
    app.local_end_turn();
    assert_eq!(
        app.local().map(|local| local.timer_left),
        Some(ENDTURN_TURN_BASED_MIN_SECONDS as f32),
        "Turn Based panel value: max(endturn, 45) (`GameManager_UpdateGui`)"
    );

    let ticks = run_ai_turn(&mut app, TeamId::GREEN, 5_000);
    let counter = app.state_ref().ai().players[TeamId::GREEN.0 as usize].end_turn_retry_counter;

    // The counter only advances on ticks with no pending orders (a moving unit
    // or a projectile holds it), so the turn length is ≥ 121 ticks; the value
    // that closes it is exactly the oracle threshold + 1.
    assert_eq!(
        counter, 121,
        "`end_turn_retry_counter > 120` closes the AI turn in Turn Based (ticks={ticks})"
    );
    assert!(
        (121..1_000).contains(&ticks),
        "the Turn Based AI turn closes on the counter, not on the timer (ticks={ticks})"
    );
    assert_eq!(
        app.local().map(|local| local.phase),
        Some(LocalPhase::PlayerTurn)
    );
}

/// `End Turn: None` (no setting) — the counter is the documented fallback.
#[test]
fn missing_setting_falls_back_to_the_counter() {
    let Some(mut app) = local_app() else {
        return;
    };

    app.start_local_game(&setup_vs_one_ai(0, PlayMode::SimultaneousMoves))
        .expect("real-time match without the end-turn setting");
    app.local_end_turn();
    assert_eq!(
        app.local().map(|local| local.timer_left),
        Some(0.0),
        "no setting — no timer"
    );
    assert_eq!(app.local().map(|local| local.ai_window_ticks), Some(0));

    let ticks = run_ai_turn(&mut app, TeamId::GREEN, 5_000);
    let counter = app.state_ref().ai().players[TeamId::GREEN.0 as usize].end_turn_retry_counter;

    assert_eq!(counter, 121, "fallback: `end_turn_retry_counter > 120`");
    assert!(
        (121..1_000).contains(&ticks),
        "without the setting the turn ends by the counter (ticks={ticks})"
    );
    assert_eq!(
        app.local().map(|local| local.phase),
        Some(LocalPhase::PlayerTurn)
    );
}
