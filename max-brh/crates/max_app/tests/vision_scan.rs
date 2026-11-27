//! Frame-side vision: the radar of a moving unit must move with the **sprite**,
//! not with the simulation cell, and the fog must **close** when the scanner
//! leaves (no explored memory).
//!
//! Oracle: `Access_UpdateMapStatus` (`max-cpp-port/src/access.cpp:762`) is called from
//! `UnitInfo::Move` exactly when the sprite crosses into the next cell
//! (`unitinfo.cpp:1787-1816`: `grid_x = x / GFX_MAP_TILE_SIZE` and then
//! `Access_UpdateMapStatus(this, true)` / `(old copy, false)`), so in the original the
//! scan — and with it the minimap fog and the enemy dots — moves on **arrival**.
//! `HeatMap::complete` is live (`heatmap.cpp:56-141`), `Access_OnCellHidden`
//! (`access.cpp:576`) darkens the fog and `UnitInfo::Draw(team)` hides the units of a
//! cell that just became hidden (`unitinfo.cpp:2842-2870`) — the fog is not a memory.
//!
//! The port's simulation itself walks a cell at the visual pace
//! (`max_core::MOVEMENT_CELLS_PER_SECOND`) while the frame draws the sprite on the
//! simulation position (`App::update_visual_units`), so the simulation scan and the
//! sprite cross a cell together. [`App::vision`] is the frame-side copy that follows
//! the visual anchors; it differs from the simulation only on a position write the
//! simulation did not walk (save/load, transport/state synchronization), where the
//! sprite keeps the fallback route. These tests pin that behaviour, the convergence
//! on arrival and the closing fog.

use max_app::{App, AppConfig};
use max_core::enums::UnitType;
use max_core::game::PLAYER_TEAM_COUNT;
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitId;
use max_core::universe::GameUniverse;

/// A tank of RED at (20,20) and an enemy tank of GREEN five cells to the east.
///
/// The tank's scan is 5 (`SC_ATTRI`), so the enemy is outside the scan at the
/// start and inside it after the first step — exactly the "the radar moved
/// before the unit arrived" window.
fn scenario() -> (App, UnitId, UnitId) {
    let mut universe = GameUniverse::with_setup(
        GameUniverse::default().snapshot().map().clone(),
        [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::None,
            TeamKind::None,
        ],
    );

    universe.set_player_team(TeamId::RED);

    let mover = universe
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(20, 20), 0)
        .expect("танк RED");
    let enemy = universe
        .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(25, 20), 0)
        .expect("танк GREEN");

    // The enemy is not known to RED until the scan reaches it.
    let mut enemy_unit = universe.snapshot().unit(enemy).cloned().expect("враг");
    enemy_unit.visible_to_team = [false; PLAYER_TEAM_COUNT + 1];
    universe.write_unit(enemy, enemy_unit);

    let mut app = App::new(AppConfig::default());
    app.adopt_universe(universe);

    (app, mover, enemy)
}

/// `UnitInfo::SpotByTeam` → `RadarPing` (`unitinfo.cpp:2833-2834`) →
/// `GameManager_NotifyEvent(this, 0)` (`game_manager.cpp:3458`): spotting an enemy
/// shows the `MESSAGE_BOX_NOTICE` «Enemy %s in radar range.» above the map (and in
/// the log). The stop itself is `enemy_halt` (`access.cpp:827-868`), tested in
/// `max_core/tests/enemy_halt.rs`.
#[test]
fn spotting_an_enemy_shows_the_radar_ping_notice() {
    let (mut app, mover, _) = scenario();

    app.update_visual_units(1.0 / 60.0);
    step_simulation(&mut app, mover);
    app.tick();

    assert_eq!(
        app.hud().hint.as_deref(),
        Some("Enemy Tank in radar range."),
        "сообщение radar ping (MESSAGE_BOX_NOTICE)"
    );
    assert_eq!(
        app.hud().hint_style,
        max_app::panels::MessageBoxStyle::Notice,
        "стиль сообщения — MESSAGE_BOX_NOTICE"
    );
}

/// Moves the **simulation** one cell east (the frame is not touched): this is what a
/// tick of `UnitInfo::Move` does — the pixel is snapped to the next cell center
/// (`SetPosition`), which is also the frame target (`visual_target`).
fn step_simulation(app: &mut App, mover: UnitId) {
    let mut unit = app.state_ref().unit(mover).cloned().expect("юнит");
    unit.grid = Point::new(unit.grid.x + 1, unit.grid.y);
    max_core::unit_order::update_grid_position(&mut unit);
    app.universe_mut().write_unit(mover, unit);
}

/// The scan follows the **sprite** in both directions.
///
/// A walked cell: the simulation paces the movement (`MOVEMENT_CELLS_PER_SECOND`)
/// and the frame draws the sprite on the simulation cell, so the frame scan covers
/// the enemy exactly when the simulation scan does — the radar never runs ahead.
/// An un-walked position write (save/load, transport/state synchronization) keeps the
/// fallback frame route, and the scan stays on the sprite until it arrives.
#[test]
fn the_radar_does_not_run_ahead_of_the_sprite() {
    let (mut app, mover, enemy) = scenario();

    // The frame is alive at `dt > 0` from the start (the SDL loop).
    app.update_visual_units(1.0 / 60.0);
    assert!(
        !app.state_ref()
            .unit(enemy)
            .expect("враг")
            .is_visible_to_team(TeamId::RED),
        "на старте враг вне скана"
    );
    assert!(!app.presented_visible(app.state_ref().unit(enemy).expect("враг")));

    // A walked cell: the sprite is the simulation position, so the frame scan
    // arrives with it and the enemy becomes visible at once.
    step_simulation(&mut app, mover);
    app.update_visual_units(1.0 / 60.0);

    let enemy_unit = app.state_ref().unit(enemy).expect("враг").clone();
    let ex = i32::from(enemy_unit.grid.x);
    let ey = i32::from(enemy_unit.grid.y);
    assert_eq!(
        app.visual_position(mover).map(|(x, _)| x),
        Some(21.5),
        "the sprite is drawn on the walked simulation pixel (cell 21 center)"
    );
    assert!(
        enemy_unit.is_visible_to_team(TeamId::RED),
        "the simulation scan sees the enemy after the step"
    );
    assert!(
        app.vision().covers(ex, ey),
        "the frame scan is rebuilt from the same cell"
    );
    assert!(app.presented_visible(&enemy_unit), "the enemy is drawn");

    // An un-walked jump back to (16,20): the frame has no route for it, so the
    // sprite walks at `VISUAL_CELLS_PER_SECOND` and the radar stays on the sprite —
    // the enemy is still shown until the sprite leaves its scan.
    {
        let mut unit = app.state_ref().unit(mover).cloned().expect("юнит");
        unit.grid = Point::new(16, 20);
        max_core::unit_order::update_grid_position(&mut unit);
        app.universe_mut().write_unit(mover, unit);
    }
    app.update_visual_units(1.0 / 60.0);
    assert!(
        app.presented_visible(&enemy_unit),
        "the radar must not leave before the sprite: the enemy stays drawn"
    );

    // The sprite arrives at the jump target without any further simulation tick
    // (`dt > 0` only); now the frame scan equals the simulation one and the enemy
    // leaves the fog.
    for _ in 0..120 {
        app.update_visual_units(1.0 / 60.0);

        if !app.presented_visible(&enemy_unit) {
            break;
        }
    }

    assert_eq!(
        app.visual_position(mover).map(|(x, _)| x),
        Some(16.5),
        "спрайт дошёл до центра клетки"
    );
    assert!(
        !app.vision().covers(ex, ey),
        "после прибытия скан кадра ушёл вместе со спрайтом"
    );
    assert!(
        !app.presented_visible(&enemy_unit),
        "враг скрылся вместе со сканом"
    );
}

/// The fog is **live**: a unit outside the current scan is not drawn even if the
/// simulation flag still says `visible` — the original clears it through
/// `Access_OnCellHidden` → `UnitInfo::Draw(team)` (`access.cpp:576-607`,
/// `unitinfo.cpp:2842-2870`). The port must not mutate the simulation flag
/// (it is part of `snapshot_bytes`/CRC), so the frame keeps the live answer.
#[test]
fn the_fog_closes_when_the_scanner_leaves() {
    let (mut app, mover, enemy) = scenario();

    // The tank walks to (21,20); on arrival the enemy at (25,20) is in its scan.
    app.update_visual_units(1.0 / 60.0);
    step_simulation(&mut app, mover);

    for _ in 0..120 {
        app.update_visual_units(1.0 / 60.0);

        if app.presented_visible(app.state_ref().unit(enemy).expect("враг")) {
            break;
        }
    }

    let enemy_unit = app.state_ref().unit(enemy).expect("враг").clone();
    let x = i32::from(enemy_unit.grid.x);
    let y = i32::from(enemy_unit.grid.y);

    assert!(app.vision().covers(x, y), "враг в текущем скане");
    assert!(app.presented_visible(&enemy_unit), "враг виден в скане");

    // The simulation moves the scanner far away (a teleport for the frame too).
    let mut unit = app.state_ref().unit(mover).cloned().expect("юнит");
    unit.grid = Point::new(20, 30);
    max_core::unit_order::update_grid_position(&mut unit);
    app.universe_mut().write_unit(mover, unit);

    let snapshot = app.snapshot();
    app.update_visual_units(1.0 / 60.0);

    let enemy_unit = app.state_ref().unit(enemy).expect("враг").clone();

    assert!(
        enemy_unit.is_visible_to_team(TeamId::RED),
        "флаг симуляции остаётся true (его сбрасывает только оракул Draw/CRC — не трогаем)"
    );
    assert!(
        !app.vision().covers(x, y),
        "скан ушёл: клетка снова под туманом"
    );
    assert!(
        !app.presented_visible(&enemy_unit),
        "враг снова скрыт — туман закрывается, а не остаётся разведанным"
    );
    assert_eq!(
        app.snapshot(),
        snapshot,
        "кадр не меняет снапшот/CRC — правка только презентационная"
    );
}

/// A unit the simulation shows although **no** own scan covers it (spotted by
/// `Access_GetAttackTarget(..., mode=true)` → `SpotByTeam`, `access.cpp:2018`) is
/// hidden by the frame: the manual rule is «видно только то, что реально попадает
/// в зону scan». Its simulation flag stays untouched (CRC).
#[test]
fn a_unit_spotted_outside_the_scan_is_not_drawn() {
    let (mut app, _mover, enemy) = scenario();

    app.update_visual_units(1.0 / 60.0);

    let mut spotted = app.state_ref().unit(enemy).cloned().expect("враг");
    spotted.set_visible_to_team(TeamId::RED, true);
    app.universe_mut().write_unit(enemy, spotted);

    let snapshot = app.snapshot();
    app.update_visual_units(1.0 / 60.0);

    let spotted = app.state_ref().unit(enemy).cloned().expect("враг");
    let x = i32::from(spotted.grid.x);
    let y = i32::from(spotted.grid.y);

    assert!(spotted.is_visible_to_team(TeamId::RED));
    assert!(
        !app.vision().covers(x, y),
        "собственный скан до клетки не достаёт (проверка сценария)"
    );
    assert!(
        !app.presented_visible(&spotted),
        "юнит вне текущего скана не рисуется (живой туман)"
    );
    assert_eq!(
        app.snapshot(),
        snapshot,
        "флаг симуляции не меняется — только презентация"
    );
}

/// `dt = 0` (QA/headless/golden frames) — the frame-side vision is the simulation
/// one: the golden frames must not change.
#[test]
fn zero_frame_time_reads_the_simulation_scan_verbatim() {
    let (mut app, mover, enemy) = scenario();

    app.update_visual_units(1.0 / 60.0);
    step_simulation(&mut app, mover);
    app.update_visual_units(0.0);

    let enemy_unit = app.state_ref().unit(enemy).expect("враг").clone();

    assert!(
        !app.vision().follow_visual(),
        "dt = 0 — маски копируются из симуляции"
    );
    assert_eq!(
        app.presented_visible(&enemy_unit),
        enemy_unit.is_visible_to_team(TeamId::RED),
        "headless-кадр показывает ровно то, что видит симуляция"
    );

    let x = i32::from(enemy_unit.grid.x) as usize;
    let y = i32::from(enemy_unit.grid.y) as usize;
    let heat = app
        .state_ref()
        .ai()
        .heat_maps
        .get(usize::from(TeamId::RED.0))
        .map(|map| map.cells()[y * max_core::geo::MAP_WIDTH as usize + x].complete > 0)
        .unwrap_or(false);

    assert_eq!(
        app.vision()
            .covers(i32::from(enemy_unit.grid.x), i32::from(enemy_unit.grid.y)),
        heat,
        "туман dt = 0 — живая heat map команды"
    );
}
