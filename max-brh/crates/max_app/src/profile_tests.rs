use super::*;

/// The counters are global: tests that touch them run one at a time.
fn lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn scope_without_enable_is_free_and_records_nothing() {
    let _guard = lock();
    // `enabled()` may be true from another test's env — reset it.
    FORCED.store(false, Ordering::Relaxed);
    reset();

    if enabled() {
        // MAX_TICK_PROFILE env is set in this environment — the test is not representative.
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
        // A fast `finish_turn` must not override the maximum and its turn.
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
