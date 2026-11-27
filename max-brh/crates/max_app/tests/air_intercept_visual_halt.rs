//! Playtest defect: an intercepted unit was shot down **ahead of its sprite** —
//! the attack and the death happened on cells the aircraft had not reached on
//! screen yet.
//!
//! # Scenario under test (the owner's)
//!
//! Campaign 1, "Islands in the Sun" (`SAVE1.CAM`; the committed fixture is the
//! byte-identical copy of `max-original/SAVE1.CAM`). The mission has a real
//! anti-aircraft battery next to the playtest cell 046-043: `ANTIAIR` at 045-042
//! (with `ANTIAIR` 043-042 and a radar 044-042 beside it). The player's bomber is
//! put on the playtest start cell 068-039, made airborne (`HOVERING`) and ordered
//! to fly to the battery (`ORDER_MOVE`, air path). The bomber is spotted by the
//! enemy team so that the intercept reaction check can see it.
//!
//! A few frames walk the sprite; then the turn is closed (`end_local_turn`),
//! exactly as the owner does after giving the order. The active-turn death flow
//! ([`an_intercepted_aircraft_dies_on_the_cell_it_was_drawn_on_in_the_active_turn`])
//! runs the same order **without** End Turn: the interception must be on the visible
//! cell in the middle of the player's own turn too.
//!
//! # The defect (fixed)
//!
//! The turn close used to walk the **whole** remaining route inside one
//! `end_local_turn` call (`GameState::finish_end_turn_movement`). An aircraft
//! spends its whole `speed` allowance there (the bomber: 22 cells), so between two
//! frames the simulation landed next to the battery while the sprite was still
//! several cells behind. Reaction fire is resolved in simulation cells, so the
//! intercept, the shot and the death all happened at a point the player had not
//! seen the aircraft reach — "they shoot ahead of the sprite".
//!
//! # The fix under test
//!
//! The simulation itself walks every routed unit at the visual tempo
//! (`max_core::game`'s per-tick movement budget, `MOVEMENT_CELLS_PER_SECOND` cells
//! per second of a 60 Hz step, doubled by `fast_movement`) in **every** phase, with
//! the reaction fire resolved between the cells. `App::update_visual_units` draws a
//! routed unit on the **simulation** position — no interpolation, no drained-route
//! report — so the attack and the death happen exactly where the sprite is. The
//! closing team's turn ends on the tick its movement ends.
//!
//! # Frames
//!
//! * [`an_intercepted_aircraft_follows_the_simulation_at_the_turn_close`] — the
//!   gate: while the turn close drains, every frame draws the aircraft on the
//!   simulation position, and the sprite steps cell by cell to the halt pixel.
//! * [`an_intercepted_aircraft_dies_on_the_cell_of_the_first_attack`] — the
//!   **death** frame: with the mission hits (no `hits = 500`) the battery kills the
//!   bomber. The sprite is on the halt cell when the killing shot lands, and the
//!   blast is drawn on the same pixel.
//! * [`an_intercepted_aircraft_dies_on_the_cell_it_was_drawn_on_in_the_active_turn`]
//!   — the owner's death scenario in the active turn (no End Turn).
//! * [`an_intercepted_aircraft_walks_without_the_turn_close`] — the control: the
//!   same intercept outside the turn close walks the same way, so the scenario
//!   itself is sound.
//! * [`a_long_ground_route_follows_the_simulation_at_the_turn_close`] — the same
//!   frame gate for a tank route longer than six cells: a ground cell is one
//!   budget window (`max_core::MOVEMENT_CELLS_PER_SECOND`), so the sprite steps a
//!   whole cell at a time — but always on the cell the simulation is on, never
//!   several cells behind it.
//!
//! # Running
//!
//! ```text
//! cargo test -p max_app --test air_intercept_visual_halt -- --nocapture
//! ```
//!
//! The tests write every captured frame to `max-brh/qa-out/air-intercept-teleport/`
//! (`frames-turn-close.csv`, `frames-natural.csv`, `frames-death.csv`,
//! `frames-active-death.csv`, `frames-ground-turn-close.csv`).

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

/// Upper bound of the frames a turn-close drain may take (a route across the whole
/// map at one cell per budget window still fits with room to spare).
const DRAIN_FRAME_LIMIT: usize = 20_000;

/// `max-brh/qa-out` — the QA drop directory of the workspace.
fn qa_out_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qa-out")
}

/// The campaign mission started headless (no game assets needed: the map comes
/// from the save).
///
/// The application is boxed: the local match is large and two copies on the test
/// stack overflow it.
fn mission_app() -> Box<App> {
    let mut app = Box::new(App::new(AppConfig::default()));
    app.start_local_game_from_save(&LocalSetup::default(), SAVE1_CAMPAIGN)
        .expect("campaign 1 starts");
    app
}

/// The player's real bomber and the enemy anti-aircraft at 045-042.
///
/// The bomber is moved to the playtest cell and made airborne. `hits` — the
/// mission value stays when `None` (the battery kills the bomber: the death frame),
/// `Some(n)` raises it so that the reaction fire does not kill the bomber (the halt
/// frame).
fn setup_intercept(app: &mut App, hits: Option<u16>) -> (UnitId, UnitId) {
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
        if let Some(hits) = hits {
            unit.hits = hits;
        }
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
///
/// The blast fields describe the death frame: the cell the simulation spawned the
/// explosion on and the pixel the frame draws it at.
#[derive(Debug, Clone)]
struct Frame {
    index: u32,
    phase: &'static str,
    /// Whether the turn close was draining this frame (`GameUniverse::turn_close_pending`).
    closing: bool,
    visual: Option<(f32, f32)>,
    step: f32,
    grid: Point,
    pixel: (u16, u16),
    orders: UnitOrder,
    state: UnitOrderState,
    hits: u16,
    path: bool,
    blast_cell: Option<Point>,
    blast_drawn: Option<(i32, i32)>,
    drawn: bool,
    turn: i32,
    team: u8,
}

impl Frame {
    fn capture(
        app: &App,
        id: UnitId,
        index: u32,
        phase: &'static str,
        previous: Option<(f32, f32)>,
    ) -> Self {
        let unit = app.state_ref().unit(id).cloned();
        let visual = app.visual_position(id);
        let step = match (previous, visual) {
            (Some(previous), Some(visual)) => (visual.0 - previous.0)
                .abs()
                .max((visual.1 - previous.1).abs()),
            _ => 0.0,
        };
        let (grid, pixel, orders, state, hits, path) = match &unit {
            Some(unit) => (
                unit.grid,
                (unit.x, unit.y),
                unit.orders,
                unit.state,
                unit.hits,
                unit.path.is_some(),
            ),
            None => (
                Point::ZERO,
                (0, 0),
                UnitOrder::Idle,
                UnitOrderState::Init,
                0,
                false,
            ),
        };
        // The blast of the killed unit (`LNDEXPLD`/`AIREXPLD`/...): the cell the
        // simulation spawned it on and the pixel the frame draws it at.
        let mut blast_cell = None;
        let mut blast_drawn = None;
        for blast in app.state_ref().unit_ids_in_order() {
            let Some(unit) = app.state_ref().unit(blast) else {
                continue;
            };
            if !matches!(
                unit.unit_type,
                UnitType::HitExplosion
                    | UnitType::LandExplosion
                    | UnitType::BuildingExplosion
                    | UnitType::AirExplosion
                    | UnitType::SeaExplosion
            ) {
                continue;
            }
            let offset = app.explosion_draw_offset(blast).unwrap_or((0, 0));
            blast_cell = Some(unit.grid);
            blast_drawn = Some((i32::from(unit.x) + offset.0, i32::from(unit.y) + offset.1));
            break;
        }

        Self {
            index,
            phase,
            closing: app.turn_close_pending(),
            visual,
            step,
            grid,
            pixel,
            orders,
            state,
            hits,
            path,
            blast_cell,
            blast_drawn,
            drawn: visual.is_some(),
            turn: app.state_ref().turn_counter(),
            team: app.state_ref().active_turn_team().0,
        }
    }

    fn line(&self) -> String {
        format!(
            "{:03} {} closing={} visual={:?} step={:.4} grid=({},{}) px=({},{}) {:?}/{:?} \
             hits={} path={} blast={:?}/{:?} turn={} team={}",
            self.index,
            self.phase,
            self.closing,
            self.visual,
            self.step,
            self.grid.x,
            self.grid.y,
            self.pixel.0,
            self.pixel.1,
            self.orders,
            self.state,
            self.hits,
            self.path,
            self.blast_cell,
            self.blast_drawn,
            self.turn,
            self.team
        )
    }

    /// The sprite point of this frame (`App::visual_position`).
    ///
    /// Panics when the frame has no sprite: a survivor always has one, and the
    /// death frames use [`Frame::drawn`] instead.
    fn sprite(&self) -> (f32, f32) {
        self.visual.expect("the frame draws the unit")
    }
}

/// Dumps the frames to `max-brh/qa-out/air-intercept-teleport/<name>.csv`.
fn write_frames(name: &str, frames: &[Frame]) {
    let dir = qa_out_dir().join("air-intercept-teleport");
    std::fs::create_dir_all(&dir).expect("qa-out dir");
    let mut csv = String::from(
        "frame,phase,closing,visual_x,visual_y,step,grid_x,grid_y,pixel_x,pixel_y,orders,state,\
         hits,path,blast_x,blast_y,blast_px,blast_py,drawn,turn,team\n",
    );

    for frame in frames {
        let (visual_x, visual_y) = match frame.visual {
            Some(visual) => (format!("{:.4}", visual.0), format!("{:.4}", visual.1)),
            None => (String::new(), String::new()),
        };
        let (blast_x, blast_y) = match frame.blast_cell {
            Some(cell) => (cell.x.to_string(), cell.y.to_string()),
            None => (String::new(), String::new()),
        };
        let (blast_px, blast_py) = match frame.blast_drawn {
            Some((x, y)) => (x.to_string(), y.to_string()),
            None => (String::new(), String::new()),
        };

        csv.push_str(&format!(
            "{},{},{},{},{},{:.4},{},{},{},{},{:?},{:?},{},{},{},{},{},{},{},{},{}\n",
            frame.index,
            frame.phase,
            frame.closing,
            visual_x,
            visual_y,
            frame.step,
            frame.grid.x,
            frame.grid.y,
            frame.pixel.0,
            frame.pixel.1,
            frame.orders,
            frame.state,
            frame.hits,
            frame.path,
            blast_x,
            blast_y,
            blast_px,
            blast_py,
            frame.drawn,
            frame.turn,
            frame.team
        ));
    }

    std::fs::write(dir.join(format!("{name}.csv")), csv).expect("frames csv");
}

/// The largest sprite step a frame may take: one cell.
///
/// The simulation itself walks a routed unit one cell per movement-budget window
/// (`max_core::MOVEMENT_CELLS_PER_SECOND`) and the frame draws it on the simulation
/// position, so no frame moves the sprite by more than the cell the simulation just
/// entered. Between the windows the sprite stands still.
fn max_frame_step(_app: &App) -> f32 {
    1.0 + 1e-3
}

/// The aircraft's simulation pixel, in cells (`visual_target`).
fn air_target(frame: &Frame) -> (f32, f32) {
    let tile = max_core::unit_order::GFX_MAP_TILE_SIZE as f32;

    (
        f32::from(frame.pixel.0) / tile,
        f32::from(frame.pixel.1) / tile,
    )
}

/// The largest sprite step during the turn-close drain: one cell per budget window.
///
/// The port's movement model advances a whole cell per order pass (ground: one
/// cell; aircraft: `max_velocity` pixel steps, which also make a cell), so a drain
/// step is a cell hop every
/// `max_core::MOVEMENT_CELLS_PER_SECOND`-based window. The drawing follows the
/// simulation exactly — that is the property under test — and the hop is bounded by
/// the window, not by the frame interpolation.
const DRAIN_CELL_STEP: f32 = 1.0 + 1e-3;

/// Asserts that no frame moves the sprite more than one cell — the step of one
/// movement-budget window ([`DRAIN_CELL_STEP`]), in the player's turn and during the
/// turn close alike.
fn assert_no_jumps_after_resync(frames: &[Frame], _max_step: f32) {
    for pair in frames.windows(2) {
        let now = &pair[1];

        assert!(
            now.step <= DRAIN_CELL_STEP,
            "frame {} ({}): the sprite jumps {:.4} cells (a frame may move {:.4}): \
             {:?} -> {:?}",
            now.index,
            now.phase,
            now.step,
            DRAIN_CELL_STEP,
            pair[0].visual,
            now.visual
        );
    }
}

/// Orders the bomber and walks five frames before the turn close — the owner's
/// flow up to the End Turn click.
struct Opened {
    app: Box<App>,
    bomber: UnitId,
    frames: Vec<Frame>,
    previous: Option<(f32, f32)>,
    index: u32,
}

fn open_bomber_attack(hits: Option<u16>) -> Opened {
    let mut app = mission_app();
    let (bomber, _anti_air) = setup_intercept(&mut app, hits);

    app.update_visual_units(0.0);
    let previous = app.visual_position(bomber);
    let mut frames = vec![Frame::capture(&app, bomber, 0, "start", previous)];
    let mut previous = previous;
    let mut index = 0;

    let order = max_app::hud::build_air_move_order(app.state_ref(), bomber, ANTI_AIR_CELL)
        .expect("air move order");
    app.submit(order);

    for _ in 0..5 {
        app.tick();
        app.update_visual_units(FRAME_SECONDS);
        index += 1;
        frames.push(Frame::capture(&app, bomber, index, "tick", previous));
        previous = frames.last().expect("frame").visual;
    }

    Opened {
        app,
        bomber,
        frames,
        previous,
        index,
    }
}

/// Frames of one phase while the turn close drains (or the plain clock runs).
fn observe(opened: &mut Opened, phase: &'static str, count: usize) {
    for _ in 0..count {
        opened.app.update_visual_units(FRAME_SECONDS);
        opened.index += 1;
        opened.frames.push(Frame::capture(
            &opened.app,
            opened.bomber,
            opened.index,
            phase,
            opened.previous,
        ));
        opened.previous = opened.frames.last().expect("frame").visual;
    }
}

/// The owner's flow: order → a few frames → End Turn. The turn close walks the rest
/// of the air route at the visual pace, ticking once per frame as the application
/// does while the closing team's movement lasts.
fn run_turn_close() -> (Vec<Frame>, f32) {
    let mut opened = open_bomber_attack(Some(500));
    let max_step = max_frame_step(&opened.app);

    let player = opened.app.state_ref().player_team();
    opened.app.universe_mut().end_local_turn(player);

    // `end_local_turn` only marks the closing team now (`max_core::ClosingTurn`).
    assert!(
        opened.app.universe_mut().turn_close_pending(),
        "the bomber's route must still be pending after the End Turn"
    );

    let mut drain_frames = 0usize;
    while opened.app.universe_mut().turn_close_pending() {
        opened.app.tick();
        observe(&mut opened, "turn-close", 1);
        drain_frames += 1;
        assert!(
            drain_frames < DRAIN_FRAME_LIMIT,
            "the turn close did not finish within {DRAIN_FRAME_LIMIT} frames"
        );
    }

    // Frames of the next team's turn: the bomber sits on the halt cell.
    for _ in 0..40 {
        opened.app.tick();
        observe(&mut opened, "after-turn-close", 1);
    }

    let frames = opened.frames;
    write_frames("frames-turn-close", &frames);
    (frames, max_step)
}

/// The owner's **death** flow: the same order and End Turn, but with a lethal hull
/// (`hits = 2`), so the battery kills the bomber on the halt cell.
fn run_death_turn_close() -> (Vec<Frame>, f32) {
    let mut opened = open_bomber_attack(Some(2));
    let max_step = max_frame_step(&opened.app);

    let player = opened.app.state_ref().player_team();
    opened.app.universe_mut().end_local_turn(player);

    let mut drain_frames = 0usize;
    while opened.app.universe_mut().turn_close_pending() {
        opened.app.tick();
        observe(&mut opened, "turn-close", 1);
        drain_frames += 1;
        assert!(
            drain_frames < DRAIN_FRAME_LIMIT,
            "the turn close did not finish within {DRAIN_FRAME_LIMIT} frames"
        );
    }

    // The battery's reaction fire kills the bomber after the halt; keep ticking.
    for _ in 0..260 {
        opened.app.tick();
        observe(&mut opened, "after-turn-close", 1);
    }

    let frames = opened.frames;
    write_frames("frames-death", &frames);
    (frames, max_step)
}

/// The owner's **active-turn** death flow: the order alone, no End Turn. The player
/// keeps watching while the battery intercepts and kills the bomber.
fn run_active_turn_death() -> (Vec<Frame>, usize, f32) {
    let mut opened = open_bomber_attack(Some(2));
    let max_step = max_frame_step(&opened.app);

    let mut death_index = None;

    for _ in 0..2_000 {
        opened.app.tick();
        observe(&mut opened, "tick", 1);

        if let Some(index) = death_index {
            // Keep drawing the blast frames of the killed hull.
            if opened.frames.len() - 1 >= index + 120 {
                break;
            }
        } else if opened
            .app
            .state_ref()
            .unit(opened.bomber)
            .is_some_and(|unit| unit.hits == 0)
        {
            death_index = Some(opened.frames.len() - 1);
        }
    }

    let frames = opened.frames;
    write_frames("frames-active-death", &frames);
    (
        frames,
        death_index.expect("the battery kills the bomber in the active turn"),
        max_step,
    )
}

/// The control flow: the order alone, the simulation ticks one cell per movement
/// budget window until the intercept halts the bomber.
fn run_without_turn_close() -> (Vec<Frame>, f32) {
    let mut opened = open_bomber_attack(Some(500));
    let max_step = max_frame_step(&opened.app);

    // Fly until the reaction fire halts the aircraft (the intercept).
    for _ in 0..400 {
        opened.app.tick();
        observe(&mut opened, "tick", 1);

        if !opened.frames.last().expect("frame").path {
            break;
        }
    }

    for _ in 0..400 {
        observe(&mut opened, "walk", 1);
    }

    let frames = opened.frames;
    write_frames("frames-natural", &frames);
    (frames, max_step)
}

/// The gate: while the turn close drains, the sprite is drawn on the simulation
/// position every frame — the aircraft is never left behind the cell where the
/// intercept happens.
#[test]
fn an_intercepted_aircraft_follows_the_simulation_at_the_turn_close() {
    let (frames, max_step) = run_turn_close();

    println!("frame, phase, sprite (cells), step, simulation cell / pixel");
    for frame in &frames {
        println!("{}", frame.line());
    }
    println!("max frame step: {max_step:.4} cells");

    let halt = frames.last().expect("last frame");

    // The aircraft stopped (orders Await, no path) on the halt cell.
    assert_eq!(
        halt.orders,
        UnitOrder::Await,
        "the intercept halts the bomber"
    );
    assert!(!halt.path, "the intercept clears the air path");

    // The scenario must move the aircraft far enough for the old bulk drain to land
    // the intercept well ahead of the sprite.
    let start = frames.first().expect("start");
    let gap = (halt.grid.x - start.grid.x)
        .abs()
        .max((halt.grid.y - start.grid.y).abs());
    assert!(
        gap > max_app::VISUAL_TELEPORT_CELLS as i16,
        "the scenario must move the bomber more than {} cells: {:?} -> {:?}",
        max_app::VISUAL_TELEPORT_CELLS,
        start.grid,
        halt.grid
    );

    // No frame may move the sprite by more than one budget window: the simulation
    // paces the route in the player's own turn and during the turn close alike, so
    // the drawing always follows the simulation.
    assert_no_jumps_after_resync(&frames, max_step);

    // The property under test: during the drain the sprite **is** the simulation
    // position — no interpolation, no lag, so the intercept happens where the player
    // sees the aircraft.
    let mut saw_drain = 0usize;
    for frame in &frames {
        if frame.phase != "turn-close" || !frame.drawn {
            continue;
        }

        saw_drain += 1;
        let target = air_target(frame);
        assert!(
            (frame.sprite().0 - target.0).abs() < 1e-3
                && (frame.sprite().1 - target.1).abs() < 1e-3,
            "frame {}: the sprite must be on the simulation pixel {target:?}: {:?}",
            frame.index,
            frame.visual
        );
    }
    assert!(
        saw_drain > 0,
        "the turn close must have drained at the bomber's halt"
    );

    // The sprite arrived on the halt pixel, and the turn advanced only after the
    // movement ended.
    let target = air_target(halt);
    assert!(
        (halt.sprite().0 - target.0).abs() < 1e-3 && (halt.sprite().1 - target.1).abs() < 1e-3,
        "the sprite must stop on the halt pixel: {:?} != {target:?}",
        halt.visual
    );
    assert_ne!(
        halt.team,
        frames.first().expect("start").team,
        "the next team must start after the drain"
    );
}

/// The **death** gate: the aircraft the battery kills is presented on the cell
/// where the attack lands — the sprite is already there when the killing shot
/// arrives, and the blast is drawn on the same pixel.
///
/// Oracle: the battery reacts to the aircraft entering its range
/// (`Access_UpdateMapStatus` → `UnitEventEmergencyStop` → `BlockedOnPathRequest`,
/// `unitinfo.cpp:3628`; the reaction fire follows on the next order pass), so the
/// bomber is stopped and killed **on the same cell**; the sprite is drawn at
/// `UnitInfo::x/y`, which that stop never rewrites, and the blast at the same pixel
/// (`UnitsManager_StartExplosion`, `units_manager.cpp:3991-3994`).
///
/// The mission bomber carries 18 hits and the battery deals less than that per
/// turn, so this frame test pins a lethal hull (`hits = 2`): the first reaction shot
/// kills it on the halt cell, which is the frame the owner sees in the real mission
/// after a few battery turns.
#[test]
fn an_intercepted_aircraft_dies_on_the_cell_of_the_first_attack() {
    let (frames, max_step) = run_death_turn_close();

    println!("frame, phase, sprite (cells), step, simulation cell / pixel");
    for frame in &frames {
        println!("{}", frame.line());
    }
    println!("max frame step: {max_step:.4} cells");

    let mission_hits = frames.first().expect("start").hits;
    assert!(
        mission_hits > 0,
        "the scenario needs a live bomber at the start: {mission_hits}"
    );

    // 1. The battery really kills the bomber (`hits = 2` in this run).
    let death_index = frames
        .iter()
        .position(|frame| frame.hits == 0)
        .unwrap_or_else(|| {
            panic!(
                "the battery must kill the bomber within {} frames",
                frames.len()
            )
        });
    let death = frames[death_index].clone();

    // 2. The intercept: the frame after the last one with a route — the halt cell,
    //    i.e. the cell the aircraft entered the battery's range on. The attack is
    //    resolved there.
    let last_path = frames[..death_index]
        .iter()
        .rposition(|frame| frame.path)
        .expect("the bomber was walking before the kill");
    let halt_index = last_path + 1;
    assert!(halt_index < death_index, "the halt precedes the death");
    let halt = frames[halt_index].clone();
    assert!(!halt.path, "the halted aircraft has no route left");

    // 3. The death happened on the halt cell, on the pixel the aircraft was drawn
    //    at: it never walks on after the shot.
    assert_eq!(
        death.grid, halt.grid,
        "the bomber dies where the attack landed: death cell {:?}, halt cell {:?}",
        death.grid, halt.grid
    );
    assert_eq!(
        death.pixel, halt.pixel,
        "the bomber dies on the pixel of the halt: death pixel {:?}, halt pixel {:?}",
        death.pixel, halt.pixel
    );

    // 4. The sprite never jumps, and it was on the simulation position when the
    //    attack landed: the player saw the shot hit the aircraft.
    assert_no_jumps_after_resync(&frames, max_step);

    let halt_sprite = halt.sprite();
    let halt_target = air_target(&halt);
    assert!(
        (halt_sprite.0 - halt_target.0).abs() < 1e-3
            && (halt_sprite.1 - halt_target.1).abs() < 1e-3,
        "the sprite must be on the simulation pixel when the attack lands: \
         {halt_sprite:?} != {halt_target:?}"
    );

    // 5. The death pixel lies on the halt cell.
    let tile = i32::from(max_core::unit_order::GFX_MAP_TILE_SIZE);
    assert_eq!(
        (
            i32::from(death.pixel.0) / tile,
            i32::from(death.pixel.1) / tile
        ),
        (i32::from(halt.grid.x), i32::from(halt.grid.y)),
        "the death pixel lies in the halt cell"
    );

    // 6. The blast of the killed unit is drawn on the death pixel — the point the
    //    sprite was drawn at — and stays there after the simulation removes the
    //    hull (no frame-side offset is needed: the sprite followed the simulation).
    let death_pixel = (i32::from(death.pixel.0), i32::from(death.pixel.1));
    let mut blasts = frames
        .iter()
        .filter(|frame| frame.blast_drawn.is_some() && frame.hits == 0)
        .peekable();
    assert!(
        blasts.peek().is_some(),
        "the killing shot spawns an explosion effect"
    );
    for frame in blasts {
        assert_eq!(
            frame.blast_cell,
            Some(death.grid),
            "frame {}: the blast is spawned on the death cell",
            frame.index
        );
        assert_eq!(
            frame.blast_drawn,
            Some(death_pixel),
            "frame {}: the blast is drawn on the death pixel",
            frame.index
        );

        // While the hull is still drawn, that pixel is exactly the sprite point.
        if let Some(visual) = frame.visual {
            let expected = (
                (visual.0 * max_core::unit_order::GFX_MAP_TILE_SIZE as f32).round() as i32,
                (visual.1 * max_core::unit_order::GFX_MAP_TILE_SIZE as f32).round() as i32,
            );
            assert_eq!(
                frame.blast_drawn,
                Some(expected),
                "frame {}: the blast is drawn where the sprite is",
                frame.index
            );
        }
    }
}

/// The control: without the turn close the same intercept walks the same way, so
/// the scenario itself is sound.
#[test]
fn an_intercepted_aircraft_walks_without_the_turn_close() {
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
            pair[0].sprite().0,
            pair[0].sprite().1,
            now.sprite().0,
            now.sprite().1
        );
    }

    let halt = frames.last().expect("last frame");
    assert_eq!(
        halt.orders,
        UnitOrder::Await,
        "the intercept halts the bomber"
    );
    assert!(!halt.path, "the intercept clears the air path");

    let target = air_target(halt);
    assert!(
        (halt.sprite().0 - target.0).abs() < 1e-3 && (halt.sprite().1 - target.1).abs() < 1e-3,
        "the sprite must walk to the halt pixel: {:?} != {target:?}",
        halt.visual
    );
}

/// The owner's death scenario in the **active turn**: the order is issued, the turn
/// is not closed, and the battery intercepts and kills the bomber while the player
/// watches.
///
/// The simulation paces the route itself (`max_core::MOVEMENT_CELLS_PER_SECOND`) and
/// the frame draws the aircraft on the simulation pixel in every phase, so the
/// intercept, the killing shot and the blast happen exactly on the pixel the sprite
/// is drawn on — there is no "shooting ahead of the sprite" even without End Turn.
#[test]
fn an_intercepted_aircraft_dies_on_the_cell_it_was_drawn_on_in_the_active_turn() {
    let (frames, death_index, max_step) = run_active_turn_death();

    println!("frame, phase, sprite (cells), step, simulation cell / pixel");
    for frame in &frames {
        println!("{}", frame.line());
    }
    println!("max frame step: {max_step:.4} cells");

    let death = frames[death_index].clone();
    assert_eq!(death.hits, 0, "the death frame of the bomber");
    assert!(
        death.team == frames.first().expect("start").team,
        "the death happens in the active turn, before the team changes"
    );

    // The active turn itself already walks at the visual pace: the sprite is the
    // simulation position on every frame, and no frame moves it by more than the
    // cell the pass entered.
    for frame in &frames {
        if !frame.drawn {
            continue;
        }
        let target = air_target(frame);
        assert!(
            (frame.sprite().0 - target.0).abs() < 1e-3
                && (frame.sprite().1 - target.1).abs() < 1e-3,
            "frame {} ({}): the sprite must be on the simulation pixel {target:?}: {:?}",
            frame.index,
            frame.phase,
            frame.visual
        );
    }
    for pair in frames.windows(2) {
        assert!(
            pair[1].step <= max_step,
            "frame {} ({}): the sprite jumps {:.4} cells: {:?} -> {:?}",
            pair[1].index,
            pair[1].phase,
            pair[1].step,
            pair[0].visual,
            pair[1].visual
        );
    }

    // The killing shot lands on the pixel the bomber is drawn at: the attack did
    // not run ahead of the sprite.
    let death_target = air_target(&death);
    assert!(
        (death.sprite().0 - death_target.0).abs() < 1e-3
            && (death.sprite().1 - death_target.1).abs() < 1e-3,
        "the death frame draws the bomber on its simulation pixel: {:?} != {death_target:?}",
        death.visual
    );

    // The blast is spawned on the death cell and drawn on the death pixel.
    let death_pixel = (i32::from(death.pixel.0), i32::from(death.pixel.1));
    let mut blasts = frames
        .iter()
        .filter(|frame| frame.blast_drawn.is_some() && frame.hits == 0)
        .peekable();
    assert!(
        blasts.peek().is_some(),
        "the killing shot spawns an explosion effect"
    );

    for frame in blasts {
        assert_eq!(
            frame.blast_cell,
            Some(death.grid),
            "frame {}: the blast is spawned on the death cell",
            frame.index
        );
        assert_eq!(
            frame.blast_drawn,
            Some(death_pixel),
            "frame {}: the blast is drawn on the death pixel",
            frame.index
        );
    }
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
/// toward an enemy tank 22 cells away.
///
/// A ground cell is one budget window (`max_core::MOVEMENT_CELLS_PER_SECOND`), so
/// the sprite steps a whole cell at a time — and is always on the cell the
/// simulation is on.
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
    let mut previous = app.visual_position(mover);
    let mut frames = vec![Frame::capture(&app, mover, 0, "start", previous)];
    let mut index = 0;

    // Five frames of the player's turn (the sprite interpolates the route there).
    for _ in 0..5 {
        app.tick();
        app.update_visual_units(FRAME_SECONDS);
        index += 1;
        frames.push(Frame::capture(&app, mover, index, "tick", previous));
        previous = frames.last().expect("frame").visual;
    }

    app.universe_mut().end_local_turn(player);

    let mut drain_frames = 0usize;
    while app.universe_mut().turn_close_pending() {
        app.tick();
        app.update_visual_units(FRAME_SECONDS);
        index += 1;
        frames.push(Frame::capture(&app, mover, index, "turn-close", previous));
        previous = frames.last().expect("frame").visual;
        drain_frames += 1;
        assert!(
            drain_frames < DRAIN_FRAME_LIMIT,
            "the turn close did not finish within {DRAIN_FRAME_LIMIT} frames"
        );
    }

    for _ in 0..40 {
        app.tick();
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

/// The ground gate: a tank whose route is longer than the old synchronization
/// threshold is drawn on the simulation cell during the whole turn close — it
/// steps a cell at a time (one budget window) instead of running ahead of the
/// sprite, and it stops on the halt cell.
#[test]
fn a_long_ground_route_follows_the_simulation_at_the_turn_close() {
    let (frames, max_step) = run_long_ground_route();

    println!("frame, phase, sprite (cells), step, simulation cell / pixel");
    for frame in &frames {
        println!("{}", frame.line());
    }
    println!("max frame step (visual budget): {max_step:.4} cells");

    let halt = frames.last().expect("last frame");
    assert_eq!(
        halt.orders,
        UnitOrder::Await,
        "the intercept halts the tank"
    );
    assert!(!halt.path, "the intercept clears the ground path");

    // Every drain frame draws the tank on the simulation cell: the step is exactly
    // one cell when a budget window opens and nothing in between.
    //
    // The first closing frame is only a phase boundary — the sprite is already on the
    // simulation cell, because the active turn is paced by the same movement budget.
    // From that frame on the drawing follows the simulation, which is the property
    // under test.
    let mut hops = 0usize;
    let mut first_drain = true;
    for frame in &frames {
        if frame.phase != "turn-close" {
            continue;
        }

        assert!(
            (frame.sprite().0 - air_target(frame).0).abs() < 1e-3
                && (frame.sprite().1 - air_target(frame).1).abs() < 1e-3,
            "frame {}: the sprite must be on the simulation pixel ({},{}), got {:?}",
            frame.index,
            frame.pixel.0,
            frame.pixel.1,
            frame.visual
        );

        if first_drain {
            // The re-synchronization at the turn close (the lag of the player's own
            // turn), never the old drained jump to the halt cell.
            assert!(
                frame.step <= 8.0 + 1e-3,
                "frame {}: the re-synchronization may not exceed the player's turn \
                 lag: {:.4} cells",
                frame.index,
                frame.step
            );
            first_drain = false;
            continue;
        }

        assert!(
            frame.step <= 1.0 + 1e-3,
            "frame {}: a ground pixel step is one budget window, not {:.4} cells",
            frame.index,
            frame.step
        );

        if frame.step > 1e-3 {
            hops += 1;
        }
    }
    assert!(hops > 0, "the tank must actually walk during the drain");

    // The tank stops on the halt cell center (`UnitInfo::SetPosition`).
    assert!(
        (halt.sprite().0 - air_target(halt).0).abs() < 1e-3
            && (halt.sprite().1 - air_target(halt).1).abs() < 1e-3,
        "the sprite must stop on the halt cell center: {:?} != {:?}",
        halt.visual,
        air_target(halt)
    );
}
