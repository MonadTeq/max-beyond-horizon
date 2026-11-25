//! Process memory for soak runs (P4).
//!
//! Exactly one metric is required — **resident set size** (RSS) and its peak — to
//! prove that 100+ turns do not cause linear memory growth. There are no external
//! crates: on Linux we read `/proc/self/status` (`VmRSS`/`VmHWM`), on Windows —
//! `GetProcessMemoryInfo` (psapi). On other platforms [`process_memory`]
//! returns `None`, and the logs print «unavailable» — this is not an error.
//!
//! Values are in **kilobytes** (like `VmRSS`) so that the log matches `/proc`.

/// Process memory snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessMemory {
    /// Current RSS, KiB.
    pub rss_kb: u64,
    /// Peak RSS over the process lifetime, KiB (0 — platform does not report it).
    pub peak_kb: u64,
}

impl ProcessMemory {
    /// RSS growth relative to `start`, KiB (may be negative — memory was
    /// returned to the OS).
    pub fn growth_kb(self, start: Self) -> i64 {
        self.rss_kb as i64 - start.rss_kb as i64
    }

    /// Log line: `rss=12345 KiB peak=23456 KiB`.
    pub fn line(self) -> String {
        let peak = if self.peak_kb == 0 {
            "peak=n/a".to_string()
        } else {
            format!("peak={} KiB", self.peak_kb)
        };

        format!("rss={} KiB {peak}", self.rss_kb)
    }
}

/// Current process memory; `None` — platform/source unavailable.
pub fn process_memory() -> Option<ProcessMemory> {
    platform::read()
}

/// Memory log line (`rss=… KiB peak=… KiB` or `rss=unavailable`).
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
#[allow(unsafe_code)] // the only exception to `#![deny(unsafe_code)]`: OS memory statistics
mod platform {
    use super::ProcessMemory;

    /// `PROCESS_MEMORY_COUNTERS` (`psapi.h`): the fields follow consecutively, `cb` is the
    /// size of the structure. Values are in bytes.
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

        // SAFETY: `GetCurrentProcess` returns a pseudo-handle of the current
        // process (it must not be closed), `counters` is a valid structure
        // of the required size, `cb` matches its size.
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
#[path = "mem_tests.rs"]
mod tests;
