//! Frame-side vision (item 2a): the radar of a moving unit must move with the
//! **sprite**, not with the simulation cell.
//!
//! Oracle: `Access_UpdateMapStatus` (`max-cpp-port/src/access.cpp:762`) is called from
//! `UnitInfo::Move` exactly when the sprite crosses into the next cell
//! (`unitinfo.cpp:1787-1816`: `grid_x = x / GFX_MAP_TILE_SIZE` and then
//! `Access_UpdateMapStatus(this, true)` / `(old copy, false)`), so in the original the
//! scan — and with it the minimap fog and the enemy dots — moves on **arrival**.
//!
//! The port's simulation advances a whole cell per tick while the frame walks the same
//! cells at `VISUAL_CELLS_PER_SECOND` (`App::update_visual_units`), so the simulation
//! scan of a moving unit is ahead of its sprite. [`App::vision`] is the frame-side
//! copy that follows the visual anchors; these tests pin the difference and the
//! convergence on arrival.

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

/// Moves the **simulation** cell one step east (the frame is not touched): this is
/// what a 60 Hz tick does while the sprite still walks the previous segment.
fn step_simulation(app: &mut App, mover: UnitId) {
    let mut unit = app.state_ref().unit(mover).cloned().expect("юнит");
    unit.grid = Point::new(unit.grid.x + 1, unit.grid.y);
    app.universe_mut().write_unit(mover, unit);
}

/// The simulation scan reveals the enemy on the tick its cell arrives, while the
/// sprite is still in the previous cell: the frame must **not** show the enemy
/// yet, and must show it once the sprite arrives.
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

    // One simulation cell = one tick; the frame gets the same `dt`, so the sprite
    // stays in the old cell (`VisualStep` budget is `VISUAL_CELLS_PER_SECOND`).
    step_simulation(&mut app, mover);
    app.update_visual_units(1.0 / 60.0);

    let enemy_unit = app.state_ref().unit(enemy).expect("враг").clone();
    let (visual_x, _) = app.visual_position(mover).expect("юнит");
    assert!(
        visual_x < 20.5,
        "спрайт ещё не прибыл в клетку 21: {visual_x}"
    );
    assert!(
        enemy_unit.is_visible_to_team(TeamId::RED),
        "симуляционный скан уже видит врага (расхождение, которое и вызывало дефект)"
    );

    // THE REGRESSION: the frame must not show the enemy ahead of the sprite.
    let ex = i32::from(enemy_unit.grid.x);
    let ey = i32::from(enemy_unit.grid.y);

    assert!(
        !app.vision().covers(ex, ey),
        "скан кадра не должен уезжать вперёд (визуальный якорь — клетка 20)"
    );
    assert!(
        !app.presented_visible(&enemy_unit),
        "враг не должен рисоваться до прибытия спрайта"
    );
    assert!(
        !app.vision().explored(ex, ey),
        "туман на миникарте не должен открываться раньше прибытия"
    );

    // The sprite catches up without any further simulation tick (`dt > 0` only).
    for _ in 0..120 {
        app.update_visual_units(1.0 / 60.0);

        if app.presented_visible(&enemy_unit) {
            break;
        }
    }

    assert_eq!(
        app.visual_position(mover).map(|(x, _)| x.round()),
        Some(21.0),
        "спрайт дошёл до клетки"
    );
    assert!(
        app.vision().covers(ex, ey),
        "после прибытия скан кадра = скан симуляции"
    );
    assert!(
        app.presented_visible(&enemy_unit),
        "после прибытия враг виден"
    );
    assert!(
        app.vision().explored(ex, ey),
        "клетка врага попала в открытый туман миникарты"
    );
}

/// A unit the simulation shows although **no** own scan covers it (spotted by fire,
/// `apply_damage` → `SpotByTeam`, `combat.rs:122`) must stay on the frame: the
/// frame-side vision only delays scan coverage, it never hides such a unit.
#[test]
fn a_unit_spotted_outside_the_scan_stays_visible() {
    let (mut app, _mover, enemy) = scenario();

    app.update_visual_units(1.0 / 60.0);

    let mut spotted = app.state_ref().unit(enemy).cloned().expect("враг");
    spotted.set_visible_to_team(TeamId::RED, true);
    app.universe_mut().write_unit(enemy, spotted);

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
        app.presented_visible(&spotted),
        "враг, замеченный огнём вне скана, не должен исчезать"
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
            .explored(i32::from(enemy_unit.grid.x), i32::from(enemy_unit.grid.y)),
        heat,
        "туман dt = 0 — heat map команды"
    );
}
