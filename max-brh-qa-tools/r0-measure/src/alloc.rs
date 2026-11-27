//! Tracking global allocator: gives exact, repeatable heap numbers for the
//! render-memory measurements (R0).
//!
//! Why an allocator and not `size_of`: the renderer holds `Vec`-backed pixel
//! buffers whose bytes dominate (frame RGBA, sprite runs, image pixels), and
//! only the allocator sees the real capacity. Measuring "before/after a load
//! phase" keeps the numbers independent of internal field visibility and of
//! the struct layout.
//!
//! The counters are process-global atomics; the tool is single-threaded, so a
//! snapshot taken between phases is stable.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Global allocator wrapper that keeps a live total and a peak.
pub struct Tracking;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

impl Tracking {
    fn add(size: usize) {
        let now = CURRENT.fetch_add(size, Ordering::Relaxed) + size;
        // Keep the peak monotonic without a lock: a relaxed compare-exchange loop.
        let mut peak = PEAK.load(Ordering::Relaxed);
        while now > peak {
            match PEAK.compare_exchange_weak(peak, now, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(observed) => peak = observed,
            }
        }
    }

    fn sub(size: usize) {
        CURRENT.fetch_sub(size, Ordering::Relaxed);
    }
}

// SAFETY: every method forwards to `System` unchanged and only observes the
// requested size, so the allocator contract (valid pointers, matching
// layouts, no double free) is preserved.
unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };

        if !pointer.is_null() {
            Self::add(layout.size());
        }

        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };

        if !pointer.is_null() {
            Self::add(layout.size());
        }

        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        Self::sub(layout.size());
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_pointer = unsafe { System.realloc(pointer, layout, new_size) };

        if !new_pointer.is_null() {
            Self::sub(layout.size());
            Self::add(new_size);
        }

        new_pointer
    }
}

/// Live heap bytes allocated by the process.
pub fn current() -> usize {
    CURRENT.load(Ordering::Relaxed)
}

/// Highest live heap size since the start (or since [`reset_peak`]).
pub fn peak() -> usize {
    PEAK.load(Ordering::Relaxed)
}

/// Drops the peak back to the live size (start of a new measurement phase).
pub fn reset_peak() {
    PEAK.store(current(), Ordering::Relaxed);
}

/// One measured phase: `label` and the heap delta while it ran.
pub struct Phase {
    /// Human-readable phase name.
    pub label: &'static str,
    /// Live heap bytes before the phase.
    pub before: usize,
    /// Live heap bytes after the phase.
    pub after: usize,
    /// Peak within the phase.
    pub peak: usize,
}

impl Phase {
    /// Bytes retained by the phase.
    pub fn retained(&self) -> usize {
        self.after.saturating_sub(self.before)
    }

    /// Peak above the starting level.
    pub fn peak_extra(&self) -> usize {
        self.peak.saturating_sub(self.before)
    }
}

/// Runs `body` as a measured phase.
pub fn phase<T>(label: &'static str, body: impl FnOnce() -> T) -> (Phase, T) {
    let before = current();
    reset_peak();
    let value = body();
    let peak = peak();
    let after = current();

    (
        Phase {
            label,
            before,
            after,
            peak,
        },
        value,
    )
}

/// Human-readable byte size (`1.17 MiB`, `17.0 MiB`, `742 KiB`).
pub fn human(bytes: usize) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let value = bytes as f64;

    if value >= MIB {
        format!("{:.2} MiB", value / MIB)
    } else if value >= KIB {
        format!("{:.1} KiB", value / KIB)
    } else {
        format!("{bytes} B")
    }
}

#[global_allocator]
static GLOBAL: Tracking = Tracking;
