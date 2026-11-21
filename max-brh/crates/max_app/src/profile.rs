//! Профилирование тика (P4, `max-brh-docs/development/PLAN-production.md`).
//!
//! Зачем: на 60 Гц кадр обязан укладываться в бюджет ~16.7 мс, а `finish_turn`
//! (`GameManager_TeamTurnFinish`) — самая тяжёлая точка хода (победа/поражение,
//! очки, производство всех команд). Модуль считает **только время**, не меняя
//! порядок и арифметику симуляции: `GameState::tick`/`finish_turn` вызываются
//! ровно там же, где раньше, а замер — это пара `Instant::now()` вокруг вызова.
//! Поэтому детерминизм (`AppDigest`, снапшоты) не затрагивается.
//!
//! Включение: переменная окружения `MAX_TICK_PROFILE=1` (или `true`/`yes`/`on`)
//! либо явный [`enable`] (так делает пример `soak_headless`, чтобы его лог
//! всегда содержал метрики). Когда профилирование выключено, [`scope`] не
//! читает часы и не трогает счётчики — обычный ход стоит один branch.
//!
//! Итог отдаёт [`summary`] — строка вида
//! `PROFILE: local_update n=… avg=… max=… over_16.67ms=…; finish_turn n=… …`.
//! Её пишут в `log.txt` QA-драйвер (`qa::runner`) и `soak_headless`.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

/// Бюджет кадра при 60 Гц, наносекунды (16.666… мс).
pub const FRAME_BUDGET_NS: u64 = 16_666_667;

/// Что замеряем.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Meter {
    /// `App::local_update` — один кадр локальной партии (тик + интерполяция).
    LocalUpdate,
    /// `GameState::finish_turn` — закрытие раунда.
    FinishTurn,
}

/// Счётчики одного замера.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MeterStats {
    /// Сколько раз вызывали.
    pub count: u64,
    /// Суммарное время, наносекунды.
    pub total_ns: u64,
    /// Максимальное время, наносекунды.
    pub max_ns: u64,
    /// Сколько вызовов дольше бюджета кадра 60 Гц (только для `local_update`).
    pub over_budget: u64,
}

impl MeterStats {
    /// Среднее время, наносекунды (`0` при `count == 0`).
    pub fn avg_ns(self) -> u64 {
        self.total_ns.checked_div(self.count).unwrap_or(0)
    }

    /// Среднее в миллисекундах.
    pub fn avg_ms(self) -> f64 {
        self.avg_ns() as f64 / 1_000_000.0
    }

    /// Максимум в миллисекундах.
    pub fn max_ms(self) -> f64 {
        self.max_ns as f64 / 1_000_000.0
    }

    /// Доля вызовов дольше бюджета кадра `0.0..=1.0`.
    pub fn over_budget_ratio(self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.over_budget as f64 / self.count as f64
        }
    }
}

/// Снимок всех счётчиков.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProfileSnapshot {
    /// `App::local_update`.
    pub local_update: MeterStats,
    /// `GameState::finish_turn`.
    pub finish_turn: MeterStats,
    /// Номер хода последнего `finish_turn` (`-1` — не было).
    pub last_finish_turn: i32,
    /// Время последнего `finish_turn`, наносекунды.
    pub last_finish_ns: u64,
    /// Номер хода самого долгого `finish_turn` (`-1` — не было).
    pub max_finish_turn: i32,
}

// ---------------------------------------------------------------------------
// Глобальные счётчики
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

/// Включено ли профилирование (env `MAX_TICK_PROFILE` или [`enable`]).
pub fn enabled() -> bool {
    FORCED.load(Ordering::Relaxed) || env_enabled()
}

/// Принудительно включает профилирование для текущего процесса.
///
/// Нужно примерам/тестам, которым метрики нужны всегда, независимо от env.
pub fn enable() {
    FORCED.store(true, Ordering::Relaxed);
}

/// Замер вокруг одного вызова. Пока профилирование выключено, часы не читаются,
/// а [`Drop`] ничего не пишет.
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

                // Максимум и его ход обновляем вместе: `fetch_max` здесь не
                // подходит, иначе к моменту чтения `FT_MAX` уже равен
                // `elapsed`, и номер хода максимума терялся бы.
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

/// Замер `App::local_update` (один кадр локальной партии).
pub fn local_update_scope() -> Scope {
    Scope {
        meter: Meter::LocalUpdate,
        started: enabled().then(Instant::now),
        turn: -1,
    }
}

/// Замер `GameState::finish_turn`; `turn` — номер закрываемого хода.
pub fn finish_turn_scope(turn: i32) -> Scope {
    Scope {
        meter: Meter::FinishTurn,
        started: enabled().then(Instant::now),
        turn,
    }
}

/// Снимок счётчиков.
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

/// Обнуляет счётчики (тесты/повторные прогоны в одном процессе).
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

/// Человекочитаемая длительность: `123ns`, `1.23us`, `12.3ms`.
pub fn format_ns(ns: u64) -> String {
    if ns < 1_000 {
        format!("{ns}ns")
    } else if ns < 1_000_000 {
        format!("{:.2}us", ns as f64 / 1_000.0)
    } else {
        format!("{:.2}ms", ns as f64 / 1_000_000.0)
    }
}

/// Одна строка со всеми метриками (для `log.txt`).
///
/// Пустая строка, если профилирование выключено и счётчики нулевые —
/// вызывающий решает, писать ли её.
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
mod tests {
    use super::*;

    /// Счётчики глобальные: тесты, трогающие их, идут по одному.
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn scope_without_enable_is_free_and_records_nothing() {
        let _guard = lock();
        // `enabled()` может быть true из env другого теста — сбрасываем.
        FORCED.store(false, Ordering::Relaxed);
        reset();

        if enabled() {
            // env MAX_TICK_PROFILE задан в этом окружении — тест не показателен.
            return;
        }

        {
            let _scope = local_update_scope();
        }

        assert_eq!(snapshot().local_update.count, 0);
        assert_eq!(snapshot().finish_turn.count, 0);
    }

    #[test]
    fn enabled_scope_records_counters_and_summary() {
        let _guard = lock();
        enable();
        reset();

        for _ in 0..3 {
            let _scope = local_update_scope();
        }
        {
            let _scope = finish_turn_scope(100);
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        {
            // Быстрый `finish_turn` не должен перебивать максимум и его ход.
            let _scope = finish_turn_scope(101);
        }

        let measured = snapshot();

        assert_eq!(measured.local_update.count, 3);
        assert_eq!(measured.finish_turn.count, 2);
        assert_eq!(measured.last_finish_turn, 101);
        assert_eq!(measured.max_finish_turn, 100, "максимум — у медленного хода");
        assert!(measured.finish_turn.max_ns >= 50_000_000, "{measured:?}");

        let text = summary();
        assert!(text.contains("local_update n=3"), "{text}");
        assert!(text.contains("finish_turn n=2"), "{text}");
        assert!(text.contains("max_at=turn100"), "{text}");

        reset();
        assert_eq!(snapshot().local_update.count, 0);
    }

    #[test]
    fn duration_formatting_uses_ns_us_ms() {
        assert_eq!(format_ns(999), "999ns");
        assert_eq!(format_ns(1_500), "1.50us");
        assert_eq!(format_ns(2_500_000), "2.50ms");
        assert_eq!(FRAME_BUDGET_NS, 16_666_667);
    }

    #[test]
    fn stats_math_is_zero_safe() {
        let empty = MeterStats::default();

        assert_eq!(empty.avg_ns(), 0);
        assert_eq!(empty.avg_ms(), 0.0);
        assert_eq!(empty.over_budget_ratio(), 0.0);

        let stats = MeterStats {
            count: 4,
            total_ns: 8_000,
            max_ns: 5_000,
            over_budget: 1,
        };

        assert_eq!(stats.avg_ns(), 2_000);
        assert_eq!(stats.max_ms(), 0.005);
        assert_eq!(stats.over_budget_ratio(), 0.25);
    }
}
