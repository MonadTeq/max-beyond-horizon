//! Память процесса для soak-прогонов (P4).
//!
//! Нужен ровно один показатель — **resident set size** (RSS) и его пик, чтобы
//! доказать, что 100+ ходов не дают линейного роста памяти. Внешних крейтов
//! нет: на Linux читаем `/proc/self/status` (`VmRSS`/`VmHWM`), на Windows —
//! `GetProcessMemoryInfo` (psapi). На прочих платформах [`process_memory`]
//! возвращает `None`, а логи пишут «unavailable» — это не ошибка.
//!
//! Значения в **килобайтах** (как `VmRSS`), чтобы лог совпадал с `/proc`.

/// Снимок памяти процесса.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessMemory {
    /// Текущий RSS, КиБ.
    pub rss_kb: u64,
    /// Пиковый RSS за жизнь процесса, КиБ (0 — платформа не сообщает).
    pub peak_kb: u64,
}

impl ProcessMemory {
    /// Рост RSS относительно `start`, КиБ (может быть отрицательным — память
    /// вернулась ОС).
    pub fn growth_kb(self, start: Self) -> i64 {
        self.rss_kb as i64 - start.rss_kb as i64
    }

    /// Строка для лога: `rss=12345 KiB peak=23456 KiB`.
    pub fn line(self) -> String {
        let peak = if self.peak_kb == 0 {
            "peak=n/a".to_string()
        } else {
            format!("peak={} KiB", self.peak_kb)
        };

        format!("rss={} KiB {peak}", self.rss_kb)
    }
}

/// Текущая память процесса; `None` — платформа/источник недоступны.
pub fn process_memory() -> Option<ProcessMemory> {
    platform::read()
}

/// Строка памяти для лога (`rss=… KiB peak=… KiB` либо `rss=unavailable`).
pub fn memory_line() -> String {
    process_memory()
        .map(|memory| memory.line())
        .unwrap_or_else(|| "rss=unavailable".to_string())
}

#[cfg(target_os = "linux")]
mod platform {
    use super::ProcessMemory;

    pub fn read() -> Option<ProcessMemory> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;

        Some(ProcessMemory {
            rss_kb: value(&status, "VmRSS:")?,
            peak_kb: value(&status, "VmHWM:").unwrap_or(0),
        })
    }

    fn value(status: &str, key: &str) -> Option<u64> {
        let line = status.lines().find(|line| line.starts_with(key))?;
        let number = line[key.len()..].split_whitespace().next()?;

        number.parse().ok()
    }
}

#[cfg(windows)]
#[allow(unsafe_code)] // единственное исключение из `#![deny(unsafe_code)]`: OS-статистика памяти
mod platform {
    use super::ProcessMemory;

    /// `PROCESS_MEMORY_COUNTERS` (`psapi.h`): поля идут подряд, `cb` — размер
    /// структуры. Значения в байтах.
    #[repr(C)]
    #[allow(non_snake_case)]
    struct ProcessMemoryCounters {
        cb: u32,
        PageFaultCount: u32,
        PeakWorkingSetSize: usize,
        WorkingSetSize: usize,
        QuotaPeakPagedPoolUsage: usize,
        QuotaPagedPoolUsage: usize,
        QuotaPeakNonPagedPoolUsage: usize,
        QuotaNonPagedPoolUsage: usize,
        PagefileUsage: usize,
        PeakPagefileUsage: usize,
    }

    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut core::ffi::c_void,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
    }

    pub fn read() -> Option<ProcessMemory> {
        let mut counters = ProcessMemoryCounters {
            cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
            PageFaultCount: 0,
            PeakWorkingSetSize: 0,
            WorkingSetSize: 0,
            QuotaPeakPagedPoolUsage: 0,
            QuotaPagedPoolUsage: 0,
            QuotaPeakNonPagedPoolUsage: 0,
            QuotaNonPagedPoolUsage: 0,
            PagefileUsage: 0,
            PeakPagefileUsage: 0,
        };

        // SAFETY: `GetCurrentProcess` возвращает псевдо-дескриптор текущего
        // процесса (его не нужно закрывать), `counters` — валидная структура
        // нужного размера, `cb` соответствует её размеру.
        let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };

        if ok == 0 {
            return None;
        }

        Some(ProcessMemory {
            rss_kb: (counters.WorkingSetSize / 1024) as u64,
            peak_kb: (counters.PeakWorkingSetSize / 1024) as u64,
        })
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod platform {
    use super::ProcessMemory;

    pub fn read() -> Option<ProcessMemory> {
        None
    }
}

#[cfg(test)]
mod tests {
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
        // Linux/Windows обязаны вернуть ненулевой RSS; другие ОС — `None`.
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
}
