//! Tick profiling (P4, `max-brh-docs/development/PLAN-production.md`).
//!
//! Why: at 60 Hz a frame must fit into a budget of ~16.7 ms, and `finish_turn`
//! (`GameManager_TeamTurnFinish`) is the heaviest point of a turn (victory/defeat,
//! points, production of all teams). The module measures **only time**, without changing
//! the order or arithmetic of the simulation: `GameState::tick`/`finish_turn` are called
//! exactly where they were before, and the measurement is a pair of `Instant::now()` around the call.
//! Determinism (`AppDigest`, snapshots) is therefore unaffected.
//!
//! Enabling: the `MAX_TICK_PROFILE=1` environment variable (or `true`/`yes`/`on`)
//! or an explicit [`enable`] (as the `soak_headless` example does, so that its log
//! always contains the metrics). When profiling is off, [`scope`] does not
//! read the clock and does not touch the counters — a regular turn costs one branch.
//!
//! The result is produced by [`summary`] — a line like
//! `PROFILE: local_update n=… avg=… max=… over_16.67ms=…; finish_turn n=… …`.
//! It is written to `log.txt` by the QA driver (`qa::runner`) and `soak_headless`.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

/// Frame budget at 60 Hz, nanoseconds (16.666… ms).
pub const FRAME_BUDGET_NS: u64 = 16_666_667;

/// What we measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Meter {
    /// `App::local_update` — one frame of a local game (tick + interpolation).
    LocalUpdate,
    /// `GameState::finish_turn` — closing a round.
    FinishTurn,
}

/// Counters of one measurement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MeterStats {
    /// How many times it was called.
    pub count: u64,
    /// Total time, nanoseconds.
    pub total_ns: u64,
    /// Maximum time, nanoseconds.
    pub max_ns: u64,
    /// How many calls are longer than the 60 Hz frame budget (only for `local_update`).
    pub over_budget: u64,
}

impl MeterStats {
    /// Average time, nanoseconds (`0` when `count == 0`).
    pub fn avg_ns(self) -> u64 {
        self.total_ns.checked_div(self.count).unwrap_or(0)
    }

    /// Average in milliseconds.
    pub fn avg_ms(self) -> f64 {
        self.avg_ns() as f64 / 1_000_000.0
    }

    /// Maximum in milliseconds.
    pub fn max_ms(self) -> f64 {
        self.max_ns as f64 / 1_000_000.0
    }

    /// Fraction of calls longer than the frame budget `0.0..=1.0`.
    pub fn over_budget_ratio(self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.over_budget as f64 / self.count as f64
        }
    }
}

/// Snapshot of all counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProfileSnapshot {
    /// `App::local_update`.
    pub local_update: MeterStats,
    /// `GameState::finish_turn`.
    pub finish_turn: MeterStats,
    /// Turn number of the last `finish_turn` (`-1` — none).
    pub last_finish_turn: i32,
    /// Time of the last `finish_turn`, nanoseconds.
    pub last_finish_ns: u64,
    /// Turn number of the longest `finish_turn` (`-1` — none).
    pub max_finish_turn: i32,
}

// ---------------------------------------------------------------------------
// Global counters
// ---------------------------------------------------------------------------

static FORCED: AtomicBool = AtomicBool::new(false);

static LU_COUNT: AtomicU64 = AtomicU64::new(0);
static LU_TOTAL: AtomicU64 = AtomicU64::new(0);
static LU_MAX: AtomicU64 = AtomicU64::new(0);
static LU_OVER: AtomicU64 = AtomicU64::new(0);

static FT_COUNT: AtomicU64 = AtomicU64::new(0);
static FT_TOTAL: AtomicU64 = AtomicU64::new(0);
static FT_MAX: AtomicU64 = AtomicU64::new(0);
static FT_LAST_NS: AtomicU64 = AtomicU64::new(0);
static FT_LAST_TURN: AtomicI32 = AtomicI32::new(-1);
static FT_MAX_TURN: AtomicI32 = AtomicI32::new(-1);

fn env_enabled() -> bool {
    static ENV: OnceLock<bool> = OnceLock::new();

    *ENV.get_or_init(|| {
        std::env::var("MAX_TICK_PROFILE")
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
    })
}

/// Whether profiling is enabled (env `MAX_TICK_PROFILE` or [`enable`]).
pub fn enabled() -> bool {
    FORCED.load(Ordering::Relaxed) || env_enabled()
}

/// Force-enables profiling for the current process.
///
/// Needed by examples/tests that require the metrics always, regardless of env.
pub fn enable() {
    FORCED.store(true, Ordering::Relaxed);
}

/// Measurement around one call. While profiling is off the clock is not read,
/// and [`Drop`] writes nothing.
#[must_use = "замер пишется при Drop — держите Scope живым до конца вызова"]
pub struct Scope {
    meter: Meter,
    started: Option<Instant>,
    turn: i32,
}

impl Drop for Scope {
    fn drop(&mut self) {
        let Some(started) = self.started else {
            return;
        };
        let elapsed = started.elapsed().as_nanos() as u64;

        match self.meter {
            Meter::LocalUpdate => {
                record(&LU_COUNT, &LU_TOTAL, &LU_MAX, elapsed);
                if elapsed > FRAME_BUDGET_NS {
                    LU_OVER.fetch_add(1, Ordering::Relaxed);
                }
            }
            Meter::FinishTurn => {
                FT_COUNT.fetch_add(1, Ordering::Relaxed);
                FT_TOTAL.fetch_add(elapsed, Ordering::Relaxed);
                FT_LAST_NS.store(elapsed, Ordering::Relaxed);
                FT_LAST_TURN.store(self.turn, Ordering::Relaxed);

                // Update the maximum and its turn together: `fetch_max` does not
                // work here, otherwise by the time `FT_MAX` is read it already equals
                // `elapsed`, and the turn number of the maximum would be lost.
                let mut current = FT_MAX.load(Ordering::Relaxed);
                loop {
                    if elapsed <= current {
                        break;
                    }
                    match FT_MAX.compare_exchange_weak(
                        current,
                        elapsed,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => {
                            FT_MAX_TURN.store(self.turn, Ordering::Relaxed);
                            break;
                        }
                        Err(actual) => current = actual,
                    }
                }
            }
        }
    }
}

fn record(count: &AtomicU64, total: &AtomicU64, max: &AtomicU64, elapsed: u64) {
    count.fetch_add(1, Ordering::Relaxed);
    total.fetch_add(elapsed, Ordering::Relaxed);
    max.fetch_max(elapsed, Ordering::Relaxed);
}

/// Measurement of `App::local_update` (one frame of a local game).
pub fn local_update_scope() -> Scope {
    Scope {
        meter: Meter::LocalUpdate,
        started: enabled().then(Instant::now),
        turn: -1,
    }
}

/// Measurement of `GameState::finish_turn`; `turn` is the number of the turn being closed.
pub fn finish_turn_scope(turn: i32) -> Scope {
    Scope {
        meter: Meter::FinishTurn,
        started: enabled().then(Instant::now),
        turn,
    }
}

/// Counters snapshot.
pub fn snapshot() -> ProfileSnapshot {
    ProfileSnapshot {
        local_update: MeterStats {
            count: LU_COUNT.load(Ordering::Relaxed),
            total_ns: LU_TOTAL.load(Ordering::Relaxed),
            max_ns: LU_MAX.load(Ordering::Relaxed),
            over_budget: LU_OVER.load(Ordering::Relaxed),
        },
        finish_turn: MeterStats {
            count: FT_COUNT.load(Ordering::Relaxed),
            total_ns: FT_TOTAL.load(Ordering::Relaxed),
            max_ns: FT_MAX.load(Ordering::Relaxed),
            over_budget: 0,
        },
        last_finish_turn: FT_LAST_TURN.load(Ordering::Relaxed),
        last_finish_ns: FT_LAST_NS.load(Ordering::Relaxed),
        max_finish_turn: FT_MAX_TURN.load(Ordering::Relaxed),
    }
}

/// Resets the counters (tests/repeated runs in one process).
pub fn reset() {
    LU_COUNT.store(0, Ordering::Relaxed);
    LU_TOTAL.store(0, Ordering::Relaxed);
    LU_MAX.store(0, Ordering::Relaxed);
    LU_OVER.store(0, Ordering::Relaxed);
    FT_COUNT.store(0, Ordering::Relaxed);
    FT_TOTAL.store(0, Ordering::Relaxed);
    FT_MAX.store(0, Ordering::Relaxed);
    FT_LAST_NS.store(0, Ordering::Relaxed);
    FT_LAST_TURN.store(-1, Ordering::Relaxed);
    FT_MAX_TURN.store(-1, Ordering::Relaxed);
}

/// Human-readable duration: `123ns`, `1.23us`, `12.3ms`.
pub fn format_ns(ns: u64) -> String {
    if ns < 1_000 {
        format!("{ns}ns")
    } else if ns < 1_000_000 {
        format!("{:.2}us", ns as f64 / 1_000.0)
    } else {
        format!("{:.2}ms", ns as f64 / 1_000_000.0)
    }
}

/// One line with all metrics (for `log.txt`).
///
/// An empty string if profiling is off and the counters are zero —
/// the caller decides whether to write it.
pub fn summary() -> String {
    let snapshot = snapshot();
    let lu = snapshot.local_update;
    let ft = snapshot.finish_turn;

    if lu.count == 0 && ft.count == 0 {
        return "PROFILE: нет замеров (MAX_TICK_PROFILE=1 включает профиль)".to_string();
    }

    let mut text = format!(
        "PROFILE: local_update n={} avg={} max={} over_16.67ms={} ({:.3}%); finish_turn n={} avg={} max={}",
        lu.count,
        format_ns(lu.avg_ns()),
        format_ns(lu.max_ns),
        lu.over_budget,
        lu.over_budget_ratio() * 100.0,
        ft.count,
        format_ns(ft.avg_ns()),
        format_ns(ft.max_ns)
    );

    if ft.count > 0 {
        text.push_str(&format!(
            " last={}@turn{} max_at=turn{}",
            format_ns(snapshot.last_finish_ns),
            snapshot.last_finish_turn,
            snapshot.max_finish_turn
        ));
    }

    text
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
