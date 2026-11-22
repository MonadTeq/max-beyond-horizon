//! Headless soak run (P4, `max-brh-docs/development/PLAN-production.md`).
//!
//! Idea: one local match "human (RED, does nothing) + N
//! computers", where each round is closed by the human's `local_end_turn`, and AI
//! turns are advanced by [`App::local_update`] with the real `dt = 1/60` — exactly like
//! the frames of the desktop loop (`main.rs`). The goal is to run 100+ turns and take
//! the metrics:
//!
//! * the time of each round and each frame (`profile`);
//! * process RSS at the start/end and every `sample_every` turns (`mem`);
//! * the fact that the match reached the requested turn (without panics and `Game over`).
//!
//! The module is the library part, so it is also used by the
//! `examples/soak_headless.rs` example (CLI in CI) and the integration test (a short
//! run). Determinism: `dt` is used only for the frame timer/interpolation,
//! the simulation still does one tick per frame.

use std::time::{Duration, Instant};

use max_core::enums::VictoryType;
use max_core::team::TeamKind;

use crate::mem::{process_memory, ProcessMemory};
use crate::profile::{self, ProfileSnapshot};
use crate::{App, LocalPhase, LocalSetup};

/// Soak run parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoakOptions {
    /// How many rounds (turns) to play.
    pub turns: u32,
    /// Frame `dt`: `1/60` — desktop interactive; `0.0` — headless like QA.
    pub dt: f32,
    /// How many computers play (1..=3).
    pub ai_teams: u8,
    /// Planet (`settings["world"]`).
    pub world: i64,
    /// Map generation seed.
    pub seed: u32,
    /// AI difficulty (`settings["opponent"]`, 0 — the easiest).
    pub opponent: u8,
    /// Turn limit of the victory condition (must be greater than `turns`, otherwise Duration
    /// will close the match earlier).
    pub victory_limit: i32,
    /// Starting credits.
    pub start_gold: u32,
    /// How many frames the human "plays" before End Turn (activity during their turn).
    pub human_ticks: u32,
    /// Game mode (`settings["play_mode"]`): `TurnBased` — like the Options item
    /// «Turn Based», `SimultaneousMoves` — the former soak mode.
    pub play_mode: max_core::enums::PlayMode,
    /// RSS sampling period in turns (`0` — start/end only).
    pub sample_every: u32,
    /// Safety fuse: maximum frames per round.
    pub frame_limit_per_turn: u32,
}

impl Default for SoakOptions {
    fn default() -> Self {
        Self {
            turns: 120,
            dt: 1.0 / 60.0,
            ai_teams: 2,
            world: 0,
            seed: 0,
            opponent: 0,
            // Duration 200: a 120-turn soak must not close the match with the limit.
            victory_limit: 200,
            start_gold: 150,
            human_ticks: 5,
            // Owner instruction: priority — Turn Based.
            play_mode: max_core::enums::PlayMode::TurnBased,
            sample_every: 10,
            frame_limit_per_turn: 50_000,
        }
    }
}

/// One completed round.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoakTurn {
    /// Number of the closed turn (`GameState::turn_counter` at closing time).
    pub turn: i32,
    /// Round time (human turn + AI turns + `finish_turn`).
    pub wall: Duration,
    /// `local_update` frames per round.
    pub frames: u64,
    /// Units in the state at the end of the round.
    pub units: usize,
    /// RSS after the round (if sampled).
    pub rss_kb: Option<u64>,
}

/// Result of the soak run.
#[derive(Debug, Clone, PartialEq)]
pub struct SoakReport {
    /// With what parameters it was run.
    pub options: SoakOptions,
    /// How many rounds were actually closed.
    pub turns_played: u32,
    /// Total `local_update` frames.
    pub frames: u64,
    /// Total run time.
    pub total: Duration,
    /// Rounds (one per closed turn).
    pub turns: Vec<SoakTurn>,
    /// Memory at the start.
    pub mem_start: Option<ProcessMemory>,
    /// Memory at the end.
    pub mem_end: Option<ProcessMemory>,
    /// Tick profile snapshot.
    pub profile: ProfileSnapshot,
    /// The match ended before the requested turn (victory/defeat).
    pub finished_early: bool,
    /// Final `turn_counter`.
    pub final_turn: i32,
    /// Final number of units.
    pub final_units: usize,
}

impl SoakReport {
    /// Average round time.
    pub fn avg_turn(&self) -> Duration {
        if self.turns_played == 0 {
            return Duration::ZERO;
        }

        self.total / self.turns_played
    }

    /// The match reached the requested turn (the main soak criterion).
    pub fn passed(&self) -> bool {
        !self.finished_early && self.turns_played >= self.options.turns
    }

    /// RSS growth "end − start", KiB (0 if the measurements are unavailable).
    pub fn memory_growth_kb(&self) -> i64 {
        match (self.mem_start, self.mem_end) {
            (Some(start), Some(end)) => end.growth_kb(start),
            _ => 0,
        }
    }

    /// Memory did not run away: growth is not more than 64 MiB (a rough leak guard).
    ///
    /// The threshold is deliberately generous: 120 turns of a live match is tens of
    /// thousands of ticks, and the allocator is entitled to keep free pages. A linear leak
    /// even of 100 KiB/turn would give 12 MiB, the real growth is a few MiB.
    pub fn memory_stable(&self) -> bool {
        self.memory_growth_kb() <= 64 * 1024
    }

    /// Log text (`soak.txt`): one line per round, memory and the result.
    pub fn log(&self) -> String {
        let options = self.options;
        let mut text = String::new();

        text.push_str(&format!(
            "# soak_headless: RED (человек, {} кадров/ход) + {} компьютер(ов)\n",
            options.human_ticks, options.ai_teams
        ));
        text.push_str(&format!(
            "# options: turns={} dt={:.2}ms ai={} world={} seed={} opponent={} mode={:?} victory=Duration/{} gold={} sample_every={} frame_limit={}\n",
            options.turns,
            options.dt * 1000.0,
            options.ai_teams,
            options.world,
            options.seed,
            options.opponent,
            options.play_mode,
            options.victory_limit,
            options.start_gold,
            options.sample_every,
            options.frame_limit_per_turn
        ));
        text.push_str(&format!(
            "# mem start: {}\n",
            memory_text(self.mem_start)
        ));

        for turn in &self.turns {
            let memory = match turn.rss_kb {
                Some(rss) => format!(" rss={rss}KiB"),
                None => String::new(),
            };

            text.push_str(&format!(
                "turn={} wall={:.2}ms frames={} units={}{memory}\n",
                turn.turn,
                turn.wall.as_secs_f64() * 1000.0,
                turn.frames,
                turn.units
            ));
        }

        text.push_str(&format!(
            "# mem end: {}\n",
            memory_text(self.mem_end)
        ));
        text.push_str(&format!(
            "# MEMORY: growth={:+} KiB stable={} (threshold 65536 KiB)\n",
            self.memory_growth_kb(),
            if self.memory_stable() { "yes" } else { "NO" }
        ));
        text.push_str(&format!(
            "SOAK RESULT: {} turns={}/{} frames={} wall={:.2}s avg_turn={:.2}ms final_turn={} units={} early_end={}\n",
            if self.passed() { "PASS" } else { "FAIL" },
            self.turns_played,
            options.turns,
            self.frames,
            self.total.as_secs_f64(),
            self.avg_turn().as_secs_f64() * 1000.0,
            self.final_turn,
            self.final_units,
            self.finished_early
        ));
        text.push_str(&profile::summary());
        text.push('\n');

        text
    }
}

fn memory_text(memory: Option<ProcessMemory>) -> String {
    memory
        .map(|memory| memory.line())
        .unwrap_or_else(|| "rss=unavailable".to_string())
}

/// Plays a soak match on an already prepared [`App`].
///
/// The assets must load (like `qa_run`/`main.rs`): `start_local_game`
/// generates the planet map. The profile is force-enabled and reset —
/// the metrics of this run are not mixed with the previous ones.
pub fn run_soak(app: &mut App, options: &SoakOptions) -> Result<SoakReport, String> {
    if options.turns == 0 {
        return Err("soak: turns должен быть > 0".to_string());
    }

    let ai_teams = options.ai_teams.clamp(1, 3);
    let mut setup = LocalSetup::default();
    setup.world = options.world;
    setup.seed = options.seed;
    setup.opponent = options.opponent;
    setup.timer = 0;
    setup.play_mode = options.play_mode;
    setup.victory_type = VictoryType::Duration;
    setup.victory_limit = options.victory_limit.max(options.turns as i32 + 1);
    setup.start_gold = options.start_gold;
    setup.team_types = [TeamKind::None; crate::LOCAL_TEAM_COUNT];
    setup.team_types[0] = TeamKind::Player;

    for slot in 1..=usize::from(ai_teams) {
        setup.team_types[slot] = TeamKind::Computer;
    }

    profile::enable();
    profile::reset();

    app.start_local_game(&setup)
        .map_err(|error| format!("soak: старт партии: {error}"))?;

    let mem_start = process_memory();
    let started = Instant::now();
    let mut turns = Vec::new();
    let mut frames = 0u64;
    let mut round_started = Instant::now();
    let mut round_frames = 0u64;
    let mut finished_early = false;
    let mut turns_played = 0u32;

    loop {
        if turns_played >= options.turns {
            break;
        }

        if app.local().is_some_and(|local| local.finished()) {
            finished_early = true;
            break;
        }

        let phase = app.local().map(|local| local.phase);

        match phase {
            Some(LocalPhase::PlayerTurn) => {
                // The turn-transition window is not opened in a single game, but
                // close it just in case — otherwise the loop would stall.
                if app
                    .local()
                    .is_some_and(|local| local.notice.is_some())
                {
                    app.local_acknowledge_turn();
                }

                let before = app.state().turn_counter;
                round_started = Instant::now();
                round_frames = 0;

                for _ in 0..options.human_ticks {
                    app.local_update(options.dt);
                    frames += 1;
                    round_frames += 1;
                }

                app.local_end_turn();

                if app.local().is_some_and(|local| local.finished())
                    || app.state().turn_counter != before
                {
                    // End Turn instantly closed the round (one team in the round).
                    turns_played += 1;
                    push_turn(
                        &mut turns,
                        app,
                        before,
                        round_started.elapsed(),
                        round_frames,
                        options,
                        turns_played,
                    );
                }
            }
            Some(_) => {
                app.local_update(options.dt);
                frames += 1;
                round_frames += 1;

                let finished = app.local().is_some_and(|local| local.finished());
                let ready = !finished
                    && matches!(
                        app.local().map(|local| local.phase),
                        Some(LocalPhase::PlayerTurn)
                    );

                if finished || ready {
                    let closed = if finished {
                        app.state().turn_counter
                    } else {
                        app.state().turn_counter.saturating_sub(1)
                    };
                    turns_played += 1;
                    push_turn(
                        &mut turns,
                        app,
                        closed,
                        round_started.elapsed(),
                        round_frames,
                        options,
                        turns_played,
                    );
                    finished_early = finished && turns_played < options.turns;

                    if finished {
                        break;
                    }
                }
            }
            None => {
                break;
            }
        }

        if frames > u64::from(options.frame_limit_per_turn) * u64::from(options.turns) {
            return Err(format!(
                "soak: предохранитель кадров сработал (frames={frames}, limit={} на раунд)",
                options.frame_limit_per_turn
            ));
        }
    }

    let total = started.elapsed();
    let mem_end = process_memory();
    let profile = profile::snapshot();
    let final_turn = app.state().turn_counter;
    let final_units = app.state().unit_count();

    Ok(SoakReport {
        options: *options,
        turns_played,
        frames,
        total,
        turns,
        mem_start,
        mem_end,
        profile,
        finished_early,
        final_turn,
        final_units,
    })
}

fn push_turn(
    turns: &mut Vec<SoakTurn>,
    app: &App,
    turn: i32,
    wall: Duration,
    frames: u64,
    options: &SoakOptions,
    played: u32,
) {
    let rss_kb = if options.sample_every > 0 && played % options.sample_every == 0 {
        process_memory().map(|memory| memory.rss_kb)
    } else {
        None
    };

    turns.push(SoakTurn {
        turn,
        wall,
        frames,
        units: app.state().unit_count(),
        rss_kb,
    });
}

/// Frame time share of the 60 Hz budget (`avg/local_update`), percent.
pub fn budget_used_percent(report: &SoakReport) -> f64 {
    let avg = report.profile.local_update.avg_ns();

    avg as f64 / profile::FRAME_BUDGET_NS as f64 * 100.0
}
