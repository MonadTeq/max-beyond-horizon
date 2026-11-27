//! Playtest defect: an **aircraft** stopped by anti-aircraft fire (the
//! `enemy_halt` intercept) teleports when the owner closes the turn, while the
//! ground regression test [`intercept_visual_halt.rs`] stays green.
//!
//! # Scenario under test (the owner's)
//!
//! Campaign 1, "Islands in the Sun" (`SAVE1.CAM`; the committed fixture is the
//! byte-identical copy of `max-original/SAVE1.CAM`). The mission has a real
//! anti-aircraft battery next to the playtest cell 046-043: `ANTIAIR` at
//! 045-042 (with `ANTIAIR` 043-042 and a radar 044-042 beside it). The player's
//! bomber is put on the playtest start cell 068-039, made airborne (`HOVERING`)
//! and ordered to fly to the battery (`ORDER_MOVE`, air path). The bomber is
//! spotted by the enemy team so that the intercept reaction check can see it.
//!
//! Frame 1..5 walk the sprite at `VISUAL_CELLS_PER_SECOND`; then the turn is
//! closed (`end_local_turn`), exactly as the owner does after giving the order.
//!
//! # Frames
//!
//! * [`an_intercepted_aircraft_walks_to_the_halt_cell_at_the_turn_close`] — the
//!   gate: after the turn close the sprite must walk the queued air route to the
//!   halt pixel, like the ground version in `intercept_visual_halt.rs`.
//! * [`an_intercepted_aircraft_walks_to_the_halt_cell_without_the_turn_close`] —
//!   the control: without the turn close the same intercept walks smoothly, so
//!   the scenario itself is sound and the bulk turn-close drain is the trigger.
//! * [`a_long_ground_route_walks_to_the_halt_cell_at_the_turn_close`] — the same
//!   frame gate for a tank route longer than six cells: it proves the defect was
//!   the drain/threshold combination, not the air path, and explains why
//!   `intercept_visual_halt.rs` (route drained by only a few cells) is green.
//!
//! # Defect (exact place)
//!
//! `App::update_visual_units` (`max_app/src/lib.rs:4295-4307` before the fix)
//! compares the frame goal with the new simulation target. A jump larger than
//! [`VISUAL_TELEPORT_CELLS`] (6 cells) is treated as a save/load or transport
//! synchronization and the sprite is set to the target without animation:
//!
//! ```ignore
//! if distance > VISUAL_TELEPORT_CELLS as f32 {
//!     queue.clear();
//!     *current = target;      // <-- the teleport
//! }
//! ```
//!
//! The jump itself is produced by the turn close:
//! `GameUniverse::end_local_turn` (`max_core/src/universe.rs:2304-2321`) calls
//! `GameState::finish_end_turn_movement` (`max_core/src/game.rs:2059-2110`),
//! which runs the still-walking routes to the end **inside one call** — the
//! oracle's `while (Access_AreTaskEventsPending()) GameManager_ProgressTurn();`
//! (`game_manager.cpp:488-490`). An aircraft spends its whole `speed` allowance
//! (bomber: 22 cells) in that drain, so the simulation target moves much farther
//! than six cells before the next `update_visual_units`; the frame had no
//! intermediate cells and applied the synchronization branch. The air path
//! itself is not at fault (`visual_target` reads the correct `AirPath` pixel,
//! `lib.rs:989-996`), the dead-unit branch is not reached (the bomber survives
//! this intercept), and `fast_movement` only doubles the per-frame budget
//! (`lib.rs:4217-4221`) — with the setting off the teleport was the same 14.6
//! cells.
//!
//! The ground test missed it because its route is drained to a halt point less
//! than six cells ahead of the sprite: the same branch was never entered. A
//! ground route longer than six drained cells teleported too (verified with a
//! 20-cell tank route), so the defect is not air-specific — the aircraft is just
//! the case where the drain always exceeds the threshold.
//!
//! # Fix
//!
//! The drain now **reports the route it walked**
//! (`GameState::finish_end_turn_movement` → [`max_core::DrainedRoute`]): every
//! order pass of the drain appends the cell and pixel the unit entered plus the
//! movement points the pass spent. `App::update_visual_units` takes the report
//! (`GameUniverse::take_drained_routes`) and — when a jump larger than
//! [`VISUAL_TELEPORT_CELLS`] ends exactly on the reported route end — rebuilds
//! the visual queue from the report instead of snapping, so the sprite walks to
//! the halt cell at `VISUAL_CELLS_PER_SECOND`. A real save/load or transport
//! synchronization has **no** report (the route is unknown) and keeps the snap.
//!
//! # Running
//!
//! ```text
//! cargo test -p max_app --test air_intercept_visual_halt -- --nocapture
//! ```
//!
//! The tests write every captured frame to `max-brh/qa-out/air-intercept-teleport/`
//! (`frames-turn-close.csv`, `frames-natural.csv`).

use std::path::{Path, PathBuf};

use max_app::local::LocalSetup;
use max_app::{App, AppConfig};
use max_core::access::unit_type_flags;
use max_core::enums::{unit_flags, Surface, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::TeamId;
use max_core::unit::{GroundPath, UnitId, UnitPath};

/// Campaign 1 mission — byte-identical to `max-original/SAVE1.CAM`.
const SAVE1_CAMPAIGN: &[u8] = include_bytes!("data/save1_v70.cam");

/// Playtest start cell of the bomber (068-039).
const BOMBER_START: Point = Point::new(68, 39);

/// The real mission anti-aircraft battery nearest to the playtest cell 046-043:
/// `ANTIAIR` at 045-042 (`SAVE1.CAM`).
const ANTI_AIR_CELL: Point = Point::new(45, 42);

/// Frame step of the visual clock (`App::update_visual_units`).
const FRAME_SECONDS: f32 = 1.0 / 60.0;

/// `max-brh/qa-out` — the QA drop directory of the workspace.
fn qa_out_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa-out")
}

/// The campaign mission started headless (no game assets needed: the map comes
/// from the save).
///
/// The application is boxed: the local match is large and two copies on the test
/// stack overflow it (the same reason `intercept_visual_halt.rs` prepares the
/// match in the caller).
fn mission_app() -> Box<App> {
    let mut app = Box::new(App::new(AppConfig::default()));
    app.start_local_game_from_save(&LocalSetup::default(), SAVE1_CAMPAIGN)
        .expect("campaign 1 starts");
    app
}

/// The player's real bomber and the enemy anti-aircraft at 045-042.
///
/// The bomber is moved to the playtest cell and made airborne; its hits are
/// raised so that the reaction fire does not kill it — the frame under test is
/// the halt, not the corpse (the death frame has its own branch and is covered
/// by `intercept_explosion.rs`).
fn setup_intercept(app: &mut App) -> (UnitId, UnitId) {
    let player = app.state_ref().player_team();

    let state = app.state_ref();
    let bomber = state
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            state
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == UnitType::Bomber && unit.team == player)
        })
        .expect("the player's bomber in campaign 1");

    let anti_air = state
        .unit_ids_in_order()
        .into_iter()
        .find(|id| {
            state.unit(*id).is_some_and(|unit| {
                unit.unit_type == UnitType::AntiAir
                    && unit.team != player
                    && unit.grid == ANTI_AIR_CELL
            })
        })
        .expect("the mission anti-aircraft at 045-042");

    let anti_air_team = state.unit(anti_air).expect("anti-air").team;

    {
        let unit = app.universe_mut().unit_mut(bomber).expect("bomber");
        unit.grid = BOMBER_START;
        unit.flags |= unit_flags::HOVERING;
        // `UnitInfo::SetPosition` for the new cell (the air path starts from `x/y`).
        max_core::unit_order::update_grid_position(unit);
        unit.hits = 500;
        unit.orders = UnitOrder::Await;
        unit.state = UnitOrderState::ExecutingOrder;
    }

    // The battery sees the bomber (the intercept check requires visibility) and
    // the halt preference is on — the default, as in the owner's campaign.
    app.universe_mut().spot_by_team(bomber, anti_air_team);
    app.universe_mut().set_enemy_halt(true);

    (bomber, anti_air)
}

/// One captured frame: the sprite point, the simulation point and the order.
#[derive(Debug, Clone)]
struct Frame {
    index: u32,
    phase: &'static str,
    visual: (f32, f32),
    step: f32,
    grid: Point,
    pixel: (u16, u16),
    orders: UnitOrder,
    state: UnitOrderState,
    hits: u16,
    path: bool,
}

impl Frame {
    fn capture(
        app: &App,
        id: UnitId,
        index: u32,
        phase: &'static str,
        previous: (f32, f32),
    ) -> Self {
        let unit = app.state_ref().unit(id).cloned().expect("bomber");
        let visual = app.visual_position(id).expect("sprite");
        let step = (visual.0 - previous.0)
            .abs()
            .max((visual.1 - previous.1).abs());

        Self {
            index,
            phase,
            visual,
            step,
            grid: unit.grid,
            pixel: (unit.x, unit.y),
            orders: unit.orders,
            state: unit.state,
            hits: unit.hits,
            path: unit.path.is_some(),
        }
    }

    fn line(&self) -> String {
        format!(
            "{:03} {} visual=({:.4},{:.4}) step={:.4} grid=({},{}) px=({},{}) {:?}/{:?} hits={} path={}",
            self.index,
            self.phase,
            self.visual.0,
            self.visual.1,
            self.step,
            self.grid.x,
            self.grid.y,
            self.pixel.0,
            self.pixel.1,
            self.orders,
            self.state,
            self.hits,
            self.path
        )
    }
}

/// Dumps the frames to `max-brh/qa-out/air-intercept-teleport/<name>.csv`.
fn write_frames(name: &str, frames: &[Frame]) {
    let dir = qa_out_dir().join("air-intercept-teleport");
    std::fs::create_dir_all(&dir).expect("qa-out dir");
    let mut csv = String::from(
        "frame,phase,visual_x,visual_y,step,grid_x,grid_y,pixel_x,pixel_y,orders,state,hits,path\n",
    );

    for frame in frames {
        csv.push_str(&format!(
            "{},{},{:.4},{:.4},{:.4},{},{},{},{},{:?},{:?},{},{}\n",
            frame.index,
            frame.phase,
            frame.visual.0,
            frame.visual.1,
            frame.step,
            frame.grid.x,
            frame.grid.y,
            frame.pixel.0,
            frame.pixel.1,
            frame.orders,
            frame.state,
            frame.hits,
            frame.path
        ));
    }

    std::fs::write(dir.join(format!("{name}.csv")), csv).expect("frames csv");
}

/// The largest sprite step a frame may take: the visual speed times the frame
/// time (with `fast_movement`, "Double Unit Steps", doubling it — the preference
/// is on by default and set in `SAVE1.CAM`).
fn max_frame_step(app: &App) -> f32 {
    let scale = if app.preferences().fast_movement {
        2.0
    } else {
        1.0
    };

    max_app::VISUAL_CELLS_PER_SECOND * scale * FRAME_SECONDS + 1e-3
}

/// The aircraft's simulation pixel at the last captured frame, in cells
/// (`visual_target`, `max_app/src/lib.rs:989-996`).
fn air_target_of_last(frames: &[Frame]) -> (f32, f32) {
    let last = frames.last().expect("last frame");
    let tile = max_core::unit_order::GFX_MAP_TILE_SIZE as f32;

    (
        f32::from(last.pixel.0) / tile,
        f32::from(last.pixel.1) / tile,
    )
}

/// The owner's flow: order → a few frames → End Turn; the turn close drains the
/// rest of the air route, then only the frame clock runs.
fn run_turn_close() -> (Vec<Frame>, f32) {
    let mut app = mission_app();
    let (bomber, _anti_air) = setup_intercept(&mut app);
    let max_step = max_frame_step(&app);

    app.update_visual_units(0.0);
    let mut previous = app.visual_position(bomber).expect("sprite");
    let mut frames = vec![Frame::capture(&app, bomber, 0, "start", previous)];

    let order = max_app::hud::build_air_move_order(app.state_ref(), bomber, ANTI_AIR_CELL)
        .expect("air move order");
    app.submit(order);

    let mut index = 0;
    for _ in 0..5 {
        app.tick();
        app.update_visual_units(FRAME_SECONDS);
        index += 1;
        frames.push(Frame::capture(&app, bomber, index, "tick", previous));
        previous = frames.last().expect("frame").visual;
    }

    let player = app.state_ref().player_team();
    app.universe_mut().end_local_turn(player);

    // The turn close drained the route inside the call above: no simulation tick
    // follows, only the frame clock (the way the UI draws after the click).
    for _ in 0..200 {
        app.update_visual_units(FRAME_SECONDS);
        index += 1;
        frames.push(Frame::capture(
            &app,
            bomber,
            index,
            "after-turn-close",
            previous,
        ));
        previous = frames.last().expect("frame").visual;
    }

    write_frames("frames-turn-close", &frames);
    (frames, max_step)
}

/// The control flow: the order alone, the simulation ticks one cell per frame
/// until the intercept halts the bomber, then the sprite walks the queue.
fn run_without_turn_close() -> (Vec<Frame>, f32) {
    let mut app = mission_app();
    let (bomber, _anti_air) = setup_intercept(&mut app);
    let max_step = max_frame_step(&app);

    app.update_visual_units(0.0);
    let mut previous = app.visual_position(bomber).expect("sprite");
    let mut frames = vec![Frame::capture(&app, bomber, 0, "start", previous)];

    let order = max_app::hud::build_air_move_order(app.state_ref(), bomber, ANTI_AIR_CELL)
        .expect("air move order");
    app.submit(order);

    let mut index = 0;

    // Fly until the reaction fire halts the aircraft (the intercept).
    for _ in 0..120 {
        app.tick();
        app.update_visual_units(FRAME_SECONDS);
        index += 1;
        frames.push(Frame::capture(&app, bomber, index, "tick", previous));
        previous = frames.last().expect("frame").visual;

        if !frames.last().expect("frame").path {
            break;
        }
    }

    // The simulation is frozen; the frame still walks the queued route.
    for _ in 0..400 {
        app.update_visual_units(FRAME_SECONDS);
        index += 1;
        frames.push(Frame::capture(&app, bomber, index, "walk", previous));
        previous = frames.last().expect("frame").visual;
    }

    write_frames("frames-natural", &frames);
    (frames, max_step)
}

/// The gate: the intercepted aircraft walks to the halt pixel after the turn
/// close, exactly like a ground unit — no frame may jump.
///
/// The turn close drains the whole remaining route inside one `end_local_turn`
/// (14.6 cells here), far past the `VISUAL_TELEPORT_CELLS` threshold. The drain
/// reports the walked waypoints (`max_core::DrainedRoute`); the frame rebuilds
/// the queue from the report, so the sprite walks to the halt pixel instead of
/// snapping to it.
#[test]
fn an_intercepted_aircraft_walks_to_the_halt_cell_at_the_turn_close() {
    let (frames, max_step) = run_turn_close();

    println!("frame, phase, sprite (cells), step, simulation cell / pixel");
    for frame in &frames {
        println!("{}", frame.line());
    }
    println!("max frame step: {max_step:.4} cells");

    // The scenario must move the aircraft farther than the synchronization
    // threshold, otherwise the frames do not exercise the defect.
    let start = frames.first().expect("start");
    let halt = frames.last().expect("last");
    let gap = (halt.visual.0 - start.visual.0)
        .abs()
        .max((halt.visual.1 - start.visual.1).abs());
    assert!(
        gap > max_app::VISUAL_TELEPORT_CELLS as f32,
        "the scenario must move the aircraft more than {} cells: {gap:.3}",
        max_app::VISUAL_TELEPORT_CELLS
    );

    // The aircraft stopped (orders Await, no path) on the halt cell.
    assert_eq!(
        halt.orders,
        UnitOrder::Await,
        "the intercept halts the bomber"
    );
    assert!(!halt.path, "the intercept clears the air path");

    // The drain must leave the sprite behind: the pre-turn-close frames are the
    // "old" goal the frame compares against.
    let last_before: Vec<&Frame> = frames
        .iter()
        .filter(|frame| frame.phase == "tick")
        .collect();
    let before = last_before.last().expect("pre-turn-close frame");
    let gap_at_close = (halt.grid.x - before.grid.x)
        .abs()
        .max((halt.grid.y - before.grid.y).abs());
    assert!(
        gap_at_close > max_app::VISUAL_TELEPORT_CELLS as i16,
        "the turn-close drain must move the bomber more than {} cells: {:?} -> {:?}",
        max_app::VISUAL_TELEPORT_CELLS,
        before.grid,
        halt.grid
    );

    let first_jump = frames.windows(2).position(|pair| pair[1].step > max_step);

    if let Some(index) = first_jump {
        let previous = &frames[index];
        let jumped = &frames[index + 1];
        panic!(
            "frame {} ({}): the sprite of the intercepted aircraft teleports {:.4} cells \
             (a frame may move {:.4}): ({:.4},{:.4}) -> ({:.4},{:.4}); the simulation halted at \
             cell ({},{}) pixel ({},{}). The turn-close drain moved the bomber into the \
             anti-aircraft range inside one `end_local_turn`, and `App::update_visual_units` \
             (`max_app/src/lib.rs:4305-4307`) treated the jump as a save/load synchronization \
             (>{} cells) instead of queuing the air route.",
            jumped.index,
            jumped.phase,
            jumped.step,
            max_step,
            previous.visual.0,
            previous.visual.1,
            jumped.visual.0,
            jumped.visual.1,
            jumped.grid.x,
            jumped.grid.y,
            jumped.pixel.0,
            jumped.pixel.1,
            max_app::VISUAL_TELEPORT_CELLS
        );
    }

    // No jump: the sprite must have arrived at the halt pixel by walking.
    let target = air_target_of_last(&frames);
    assert!(
        (halt.visual.0 - target.0).abs() < 1e-3 && (halt.visual.1 - target.1).abs() < 1e-3,
        "the sprite must stop on the halt pixel: {:?} != {target:?}",
        halt.visual
    );
}

/// The control: without the turn close the same intercept walks smoothly, so the
/// scenario is sound and the defect needs the bulk drain.
#[test]
fn an_intercepted_aircraft_walks_to_the_halt_cell_without_the_turn_close() {
    let (frames, max_step) = run_without_turn_close();

    println!("frame, phase, sprite (cells), step, simulation cell / pixel");
    for frame in &frames {
        println!("{}", frame.line());
    }
    println!("max frame step: {max_step:.4} cells");

    for pair in frames.windows(2) {
        let now = &pair[1];
        assert!(
            now.step <= max_step,
            "frame {} ({}): the sprite teleports {:.4} cells: ({:.4},{:.4}) -> ({:.4},{:.4})",
            now.index,
            now.phase,
            now.step,
            pair[0].visual.0,
            pair[0].visual.1,
            now.visual.0,
            now.visual.1
        );
    }

    let halt = frames.last().expect("last frame");
    assert_eq!(
        halt.orders,
        UnitOrder::Await,
        "the intercept halts the bomber"
    );
    assert!(!halt.path, "the intercept clears the air path");

    let target = air_target_of_last(&frames);
    assert!(
        (halt.visual.0 - target.0).abs() < 1e-3 && (halt.visual.1 - target.1).abs() < 1e-3,
        "the sprite must walk to the halt pixel: {:?} != {target:?}",
        halt.visual
    );
}

/// A horizontal run of `count` free land cells (`None` — the map has none).
fn free_row(app: &App, count: i16) -> Option<Point> {
    for y in 10..(max_core::geo::MAP_HEIGHT - 10) {
        'x: for x in 10..(max_core::geo::MAP_WIDTH - 10 - count) {
            for step in 0..count {
                let grid = Point::new(x + step, y);
                let land = app.state_ref().map().surface_at(grid.x, grid.y).bits()
                    & Surface::LAND.bits()
                    != 0;

                if !land || !app.state_ref().units_at(grid).is_empty() {
                    continue 'x;
                }
            }

            return Some(Point::new(x, y));
        }
    }

    None
}

/// The ground counterpart of [`run_turn_close`]: a tank walks a 20-cell route
/// toward an enemy tank 22 cells away, so the turn-close drain leaves it more
/// than `VISUAL_TELEPORT_CELLS` cells from the start before the intercept.
///
/// This is the same frame shape as `intercept_visual_halt.rs`; only the route is
/// long enough for the drained gap to exceed the synchronization threshold.
fn run_long_ground_route() -> (Vec<Frame>, f32) {
    let mut app = mission_app();
    let player = app.state_ref().player_team();

    for id in app.state_ref().unit_ids_in_order() {
        app.universe_mut().destroy_unit(id);
    }

    let start = free_row(&app, 26).expect("free land row");
    app.universe_mut().set_enemy_halt(true);

    let flags = unit_type_flags(UnitType::Tank);
    let mover = app
        .universe_mut()
        .spawn_unit(UnitType::Tank, player, start, flags)
        .expect("mover");
    let _enemy = app
        .universe_mut()
        .spawn_unit(
            UnitType::Tank,
            TeamId(1),
            Point::new(start.x + 22, start.y),
            flags,
        )
        .expect("enemy");
    let destination = Point::new(start.x + 20, start.y);
    {
        let unit = app.universe_mut().unit_mut(mover).expect("mover");
        unit.hits = 2000;
        unit.orders = UnitOrder::Move;
        unit.state = UnitOrderState::InProgress;
        unit.speed = 20;
        unit.path = Some(UnitPath::Ground(GroundPath {
            end_x: destination.x,
            end_y: destination.y,
            step_index: 0,
            steps: vec![max_core::geo::PathStep::new(1, 0); 20],
        }));
    }
    app.universe_mut().set_order_data(
        mover,
        max_core::game::OrderData {
            move_to: destination,
            fire_on: Point::ZERO,
        },
    );
    app.universe_mut().spot_by_team(mover, TeamId(1));
    app.universe_mut().update_visibility_status(false);

    let max_step = max_frame_step(&app);
    app.update_visual_units(0.0);
    let mut previous = app.visual_position(mover).expect("sprite");
    let mut frames = vec![Frame::capture(&app, mover, 0, "start", previous)];

    let mut index = 0;
    for _ in 0..5 {
        app.tick();
        app.update_visual_units(FRAME_SECONDS);
        index += 1;
        frames.push(Frame::capture(&app, mover, index, "tick", previous));
        previous = frames.last().expect("frame").visual;
    }

    app.universe_mut().end_local_turn(player);

    for _ in 0..220 {
        app.update_visual_units(FRAME_SECONDS);
        index += 1;
        frames.push(Frame::capture(
            &app,
            mover,
            index,
            "after-turn-close",
            previous,
        ));
        previous = frames.last().expect("frame").visual;
    }

    write_frames("frames-ground-turn-close", &frames);
    (frames, max_step)
}

/// The same gate for a ground route: a tank whose drained route is longer than
/// the synchronization threshold must walk to the halt cell, not teleport.
///
/// It reproduces the defect for a non-air unit (which is why the existing
/// `intercept_visual_halt.rs` scenario — a short drained route — is green) and
/// proves the fix uses the drained cells, not the straight line the air path
/// would allow.
#[test]
fn a_long_ground_route_walks_to_the_halt_cell_at_the_turn_close() {
    let (frames, max_step) = run_long_ground_route();

    println!("frame, phase, sprite (cells), step, simulation cell / pixel");
    for frame in &frames {
        println!("{}", frame.line());
    }
    println!("max frame step: {max_step:.4} cells");

    let halt = frames.last().expect("last frame");
    assert_eq!(
        halt.orders,
        UnitOrder::Await,
        "the intercept halts the tank"
    );
    assert!(!halt.path, "the intercept clears the ground path");

    let before = frames
        .iter()
        .filter(|frame| frame.phase == "tick")
        .last()
        .expect("pre-turn-close frame");
    let drained = (halt.grid.x - before.grid.x)
        .abs()
        .max((halt.grid.y - before.grid.y).abs());
    assert!(
        drained > max_app::VISUAL_TELEPORT_CELLS as i16,
        "the turn-close drain must move the tank more than {} cells: {:?} -> {:?}",
        max_app::VISUAL_TELEPORT_CELLS,
        before.grid,
        halt.grid
    );

    if let Some(index) = frames.windows(2).position(|pair| pair[1].step > max_step) {
        let previous = &frames[index];
        let jumped = &frames[index + 1];
        panic!(
            "frame {} ({}): the sprite of the halted tank teleports {:.4} cells (a frame may \
             move {:.4}): ({:.4},{:.4}) -> ({:.4},{:.4}); halt cell ({},{}).",
            jumped.index,
            jumped.phase,
            jumped.step,
            max_step,
            previous.visual.0,
            previous.visual.1,
            jumped.visual.0,
            jumped.visual.1,
            jumped.grid.x,
            jumped.grid.y
        );
    }

    // No jump: the ground sprite stops exactly on the halt cell.
    let expected = (f32::from(halt.grid.x), f32::from(halt.grid.y));
    assert!(
        (halt.visual.0 - expected.0).abs() < 1e-3 && (halt.visual.1 - expected.1).abs() < 1e-3,
        "the sprite must stop on the halt cell: {:?} != {expected:?}",
        halt.visual
    );
}
