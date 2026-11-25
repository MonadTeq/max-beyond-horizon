use super::*;

#[test]
fn line_contains_rss_in_kib() {
    let memory = ProcessMemory {
        rss_kb: 42_000,
        peak_kb: 64_000,
    };

    let line = memory.line();
    assert!(line.contains("rss=42000 KiB"), "{line}");
    assert!(line.contains("peak=64000 KiB"), "{line}");

    let no_peak = ProcessMemory {
        rss_kb: 1,
        peak_kb: 0,
    };
    assert!(no_peak.line().contains("peak=n/a"), "{}", no_peak.line());
}

#[test]
fn growth_is_signed() {
    let start = ProcessMemory {
        rss_kb: 10_000,
        peak_kb: 12_000,
    };
    let end = ProcessMemory {
        rss_kb: 11_500,
        peak_kb: 12_500,
    };

    assert_eq!(end.growth_kb(start), 1_500);
    assert_eq!(start.growth_kb(end), -1_500);
}

#[test]
fn current_process_memory_is_readable_when_the_platform_supports_it() {
    // Linux/Windows must return a non-zero RSS; other OSes — `None`.
    match process_memory() {
        Some(memory) => {
            #[cfg(any(target_os = "linux", windows))]
            assert!(memory.rss_kb > 0, "RSS={} KiB", memory.rss_kb);
            let _ = memory;
        }
        None => {
            #[cfg(any(target_os = "linux", windows))]
            panic!("платформа обязана уметь читать RSS");
        }
    }
}

#[test]
fn memory_line_is_never_empty() {
    let line = memory_line();
    assert!(line.starts_with("rss="), "{line}");
}
