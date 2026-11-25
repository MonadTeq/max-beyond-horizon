//! Phase 0 asset-loading benchmark (`max-brh-docs/development/asset-storage.md`).
//!
//! Measures wall time and peak heap usage of the current (unoptimised) loading paths:
//!
//! * `ResArchive::open(MAX.RES)` (17 MiB) and the raw `fs::read` baseline;
//! * the first sprite decode (`TANK`) and the first palette decode (`FRAMEPIC` - a
//!   big image carrying the 256-colour palette the client installs);
//! * `Mve::open(MAXINT.MVE)` (68 MiB) plus the first decoded frame;
//! * `WavClip::open(MAIN_MSC.MSC)` (a 17 MiB PCM16 WAV - the music path).
//!
//! Peak memory is the high-water mark of **live bytes** tracked by a counting global
//! allocator (logical allocations, not RSS); the OS working-set counters are printed
//! as a cross-check. Every scenario is repeated `--runs` times (default 3) and the
//! mean is printed. This is a measurement tool only: it does not touch the loaders.
//!
//! Usage (run a release build: debug timings are not representative):
//!
//! ```sh
//! cd max-brh
//! cargo run --release -p max_assets --example bench_assets -- --mode res
//! cargo run --release -p max_assets --example bench_assets -- --mode sprite
//! cargo run --release -p max_assets --example bench_assets -- --mode palette
//! cargo run --release -p max_assets --example bench_assets -- --mode msc
//! cargo run --release -p max_assets --example bench_assets -- --mode mve
//! # All scenarios in one process (the OS peak working set is then cumulative).
//! cargo run --release -p max_assets --example bench_assets -- --mode all
//! ```
//!
//! `--data-dir PATH` overrides the default `<repo>/max-original`.

use max_assets::image::BigImage;
use max_assets::mve::Mve;
use max_assets::res::ResArchive;
use max_assets::sprite::decode_sprite;
use max_assets::wav::WavClip;
use std::alloc::{GlobalAlloc, Layout, System};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const MIB: f64 = 1024.0 * 1024.0;
const DEFAULT_RUNS: usize = 3;
/// `FRAMEPIC` embeds the 256-colour palette at this offset (`ImageBigHeader`).
const BIG_IMAGE_PALETTE_OFFSET: usize = 8;
const PALETTE_BYTES: usize = 256 * 3;

// ---------------------------------------------------------------------------
// Counting allocator: logical live bytes and their high-water mark.
// ---------------------------------------------------------------------------

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);

/// Counts every allocation so that the peak of `LIVE_BYTES` can be reported.
struct TrackingAllocator;

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            track_growth(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc_zeroed(layout);
        if !ptr.is_null() {
            track_growth(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        System.dealloc(ptr, layout);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = System.realloc(ptr, layout, new_size);
        if !new_ptr.is_null() {
            // Logical accounting: the old block is released before the new one is counted.
            // The allocator may move the block physically, but the live-byte total is what
            // the benchmark reports.
            LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
            track_growth(new_size);
        }
        new_ptr
    }
}

fn track_growth(size: usize) {
    let live = LIVE_BYTES.fetch_add(size, Ordering::Relaxed) + size;
    PEAK_BYTES.fetch_max(live, Ordering::Relaxed);
}

#[global_allocator]
static GLOBAL: TrackingAllocator = TrackingAllocator;

fn live_bytes() -> usize {
    LIVE_BYTES.load(Ordering::Relaxed)
}

fn peak_bytes() -> usize {
    PEAK_BYTES.load(Ordering::Relaxed)
}

fn reset_peak() {
    PEAK_BYTES.store(live_bytes(), Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// OS working set (cross-check for the allocator numbers).
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod os_memory {
    use std::mem;

    /// `PROCESS_MEMORY_COUNTERS` from `psapi.h` (x64 and x86 layouts are the same
    /// because every field after the two `DWORD`s is `SIZE_T`).
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(
            process: isize,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    /// `(working_set, peak_working_set)` of the current process, in bytes.
    pub fn process_memory() -> Option<(usize, usize)> {
        let mut counters = ProcessMemoryCounters {
            cb: mem::size_of::<ProcessMemoryCounters>() as u32,
            page_fault_count: 0,
            peak_working_set_size: 0,
            working_set_size: 0,
            quota_peak_paged_pool_usage: 0,
            quota_paged_pool_usage: 0,
            quota_peak_non_paged_pool_usage: 0,
            quota_non_paged_pool_usage: 0,
            pagefile_usage: 0,
            peak_pagefile_usage: 0,
        };
        let cb = counters.cb;
        // SAFETY: `counters` is a valid, correctly sized `PROCESS_MEMORY_COUNTERS`
        // and the call only writes into it.
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, cb) };
        if ok == 0 {
            None
        } else {
            Some((counters.working_set_size, counters.peak_working_set_size))
        }
    }
}

#[cfg(not(windows))]
mod os_memory {
    /// Working-set counters are Windows-only in this tool.
    pub fn process_memory() -> Option<(usize, usize)> {
        None
    }
}

// ---------------------------------------------------------------------------
// Measurement primitives.
// ---------------------------------------------------------------------------

/// Result of one measured scope.
struct Stats {
    wall: Duration,
    live_before: usize,
    live_after: usize,
    peak: usize,
    working_set: Option<(usize, usize)>,
}

impl Stats {
    fn wall_ms(&self) -> f64 {
        self.wall.as_secs_f64() * 1_000.0
    }

    fn peak_mib(&self) -> f64 {
        mib(self.peak)
    }

    fn live_after_mib(&self) -> f64 {
        mib(self.live_after)
    }

    fn live_before_mib(&self) -> f64 {
        mib(self.live_before)
    }

    fn working_set_mib(&self) -> Option<f64> {
        self.working_set.map(|(working_set, _)| mib(working_set))
    }
}

fn mib(bytes: usize) -> f64 {
    bytes as f64 / MIB
}

/// Runs `work` and records wall time plus the live-byte high-water mark.
fn measure<T>(work: impl FnOnce() -> T) -> (T, Stats) {
    let live_before = live_bytes();
    reset_peak();
    let start = Instant::now();
    let value = work();
    let wall = start.elapsed();
    let live_after = live_bytes();
    let peak = peak_bytes().max(live_after);
    let working_set = os_memory::process_memory();

    (
        value,
        Stats {
            wall,
            live_before,
            live_after,
            peak,
            working_set,
        },
    )
}

/// Prints every run and the mean for one labelled operation.
fn report(scope: &str, label: &str, stats: &[Stats]) {
    for (index, run) in stats.iter().enumerate() {
        let working_set = run
            .working_set_mib()
            .map(|value| format!("{value:>8.2}"))
            .unwrap_or_else(|| "     n/a".to_string());
        println!(
            "[{scope}] {label} run {}/{}: {:>9.3} ms | peak {:>8.2} MiB | live before {:>8.2} MiB | live after {:>8.2} MiB | working set {working_set} MiB",
            index + 1,
            stats.len(),
            run.wall_ms(),
            run.peak_mib(),
            run.live_before_mib(),
            run.live_after_mib(),
        );
    }

    if stats.is_empty() {
        return;
    }

    let count = stats.len() as f64;
    let mean_wall: f64 = stats.iter().map(Stats::wall_ms).sum::<f64>() / count;
    let min_wall: f64 = stats
        .iter()
        .map(Stats::wall_ms)
        .fold(f64::INFINITY, f64::min);
    let mean_peak: f64 = stats.iter().map(Stats::peak_mib).sum::<f64>() / count;
    let mean_live: f64 = stats.iter().map(Stats::live_after_mib).sum::<f64>() / count;
    println!(
        "[{scope}] {label} mean: {:>9.3} ms (min {min_wall:.3} ms) | peak {mean_peak:>8.2} MiB | live after {mean_live:>8.2} MiB",
        mean_wall
    );

    // The OS peak working set is cumulative for the process, so it is meaningful when a
    // mode runs in its own process (as the driver does). It includes the runtime baseline.
    let os_peak = stats
        .iter()
        .filter_map(|run| run.working_set.map(|(_, peak)| peak))
        .max();
    if let Some(peak) = os_peak {
        println!(
            "[{scope}] {label} process peak working set (OS): {:.2} MiB",
            mib(peak)
        );
    }
}

/// `fs::read` baseline for one file: isolates file I/O from parsing.
fn bench_fs_read(scope: &str, path: &Path, runs: usize) -> Vec<Stats> {
    let mut stats = Vec::with_capacity(runs);

    for _ in 0..runs {
        let (bytes, run) = measure(|| fs::read(path));
        let bytes = bytes.unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(
            bytes.len() as u64,
            file_len(path),
            "short read of {}",
            path.display()
        );
        stats.push(run);
        drop(bytes);
    }

    report(scope, "fs::read only", &stats);
    stats
}

fn file_len(path: &Path) -> u64 {
    fs::metadata(path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        .len()
}

// ---------------------------------------------------------------------------
// Scenarios.
// ---------------------------------------------------------------------------

/// `ResArchive::open(MAX.RES)`: whole-file read + table parse.
fn bench_res(data_dir: &Path, runs: usize) {
    let path = data_dir.join("MAX.RES");
    println!(
        "[res] MAX.RES = {} bytes ({:.2} MiB)",
        file_len(&path),
        mib(file_len(&path) as usize)
    );

    bench_fs_read("res", &path, runs);

    let mut stats = Vec::with_capacity(runs);
    let mut details = String::new();

    for _ in 0..runs {
        let (archive, run) = measure(|| ResArchive::open(&path));
        let archive = archive.expect("MAX.RES opens");
        if details.is_empty() {
            details = format!(
                "entries={} table_offset={} table_size={} warnings={} data_len={}",
                archive.len(),
                archive.table_offset(),
                archive.table_size(),
                archive.warnings().len(),
                archive.file_size(),
            );
        }
        stats.push(run);
        drop(archive);
    }

    report("res", "ResArchive::open", &stats);
    println!("[res] archive: {details}");

    // `read()` is a zero-copy slice lookup: the per-call cost is the BTreeMap lookup.
    let archive = ResArchive::open(&path).expect("MAX.RES opens");
    let iterations = 10_000u32;
    let start = Instant::now();
    let mut total_len = 0u64;

    for _ in 0..iterations {
        total_len +=
            std::hint::black_box(archive.read("TANK").expect("TANK is in MAX.RES").len()) as u64;
    }

    let elapsed = start.elapsed();
    println!(
        "[res] read(\"TANK\") x{iterations}: {:.3} us/call, body {} bytes (total {total_len})",
        elapsed.as_secs_f64() * 1e6 / f64::from(iterations),
        total_len / u64::from(iterations),
    );
}

/// First sprite decode: `decode_sprite(MAX.RES:TANK)`.
fn bench_sprite(data_dir: &Path, runs: usize) {
    let path = data_dir.join("MAX.RES");
    let archive = ResArchive::open(&path).expect("MAX.RES opens");
    let tag = "TANK";
    let body_len = archive.read(tag).expect("TANK is in MAX.RES").len();
    println!("[sprite] tag={tag} body={body_len} bytes");

    let mut stats = Vec::with_capacity(runs);
    let mut details = String::new();

    for _ in 0..runs {
        let (sheet, run) =
            measure(|| decode_sprite(archive.read(tag).expect("TANK is in MAX.RES")));
        let sheet = sheet.expect("TANK decodes");
        if details.is_empty() {
            let opaque: usize = sheet.frames.iter().map(|frame| frame.opaque_pixels()).sum();
            details = format!("frames={} opaque_pixels={opaque}", sheet.len());
        }
        stats.push(run);
        drop(sheet);
    }

    report("sprite", "decode_sprite(TANK)", &stats);
    println!("[sprite] sheet: {details}");
}

/// First palette decode: the palette is embedded in `FRAMEPIC`, so the real cost is
/// `BigImage::decode(FRAMEPIC)`; the 768-byte palette copy itself is measured too.
fn bench_palette(data_dir: &Path, runs: usize) {
    let path = data_dir.join("MAX.RES");
    let archive = ResArchive::open(&path).expect("MAX.RES opens");
    let tag = "FRAMEPIC";
    let body = archive.read(tag).expect("FRAMEPIC is in MAX.RES");
    println!("[palette] tag={tag} body={} bytes", body.len());

    let mut stats = Vec::with_capacity(runs);
    let mut details = String::new();

    for _ in 0..runs {
        let (image, run) = measure(|| BigImage::decode(archive.read(tag).expect("FRAMEPIC")));
        let image = image.expect("FRAMEPIC decodes");
        if details.is_empty() {
            details = format!(
                "size={}x{} pixels={} palette_bytes={}",
                image.width,
                image.height,
                image.pixels().len(),
                image.palette.len(),
            );
        }
        stats.push(run);
        drop(image);
    }

    report("palette", "BigImage::decode(FRAMEPIC)", &stats);
    println!("[palette] image: {details}");

    // The palette alone is a 768-byte slice copy (`&bytes[8..8 + 768]`).
    let mut copy_stats = Vec::with_capacity(runs);

    for _ in 0..runs {
        let (palette, run) = measure(|| {
            let bytes = archive.read(tag).expect("FRAMEPIC is in MAX.RES");
            bytes[BIG_IMAGE_PALETTE_OFFSET..BIG_IMAGE_PALETTE_OFFSET + PALETTE_BYTES].to_vec()
        });
        assert_eq!(palette.len(), PALETTE_BYTES);
        copy_stats.push(run);
        drop(palette);
    }

    report("palette", "palette bytes copy", &copy_stats);
}

/// Music: `WavClip::open(MAIN_MSC.MSC)` - whole-file read + PCM16 decode.
fn bench_msc(data_dir: &Path, runs: usize) {
    let path = data_dir.join("MAIN_MSC.MSC");
    let len = file_len(&path);
    println!(
        "[msc] MAIN_MSC.MSC = {len} bytes ({:.2} MiB)",
        mib(len as usize)
    );

    bench_fs_read("msc", &path, runs);

    let mut stats = Vec::with_capacity(runs);
    let mut details = String::new();

    for _ in 0..runs {
        let (clip, run) = measure(|| WavClip::open(&path));
        let clip = clip.expect("MAIN_MSC.MSC decodes");
        if details.is_empty() {
            details = format!(
                "rate={} Hz channels={} samples={} frames={} duration={:.2} s",
                clip.sample_rate,
                clip.channels,
                clip.samples.len(),
                clip.frames(),
                clip.duration().as_secs_f64(),
            );
        }
        stats.push(run);
        drop(clip);
    }

    report("msc", "WavClip::open", &stats);
    println!("[msc] clip: {details}");
}

/// Intro video: `Mve::open(MAXINT.MVE)` - whole-file read + frame index, then the
/// first `next_frame()` (decoder buffers included).
fn bench_mve(data_dir: &Path, runs: usize) {
    let path = data_dir.join("MAXINT.MVE");
    let len = file_len(&path);
    println!(
        "[mve] MAXINT.MVE = {len} bytes ({:.2} MiB)",
        mib(len as usize)
    );

    bench_fs_read("mve", &path, runs);

    let mut stats = Vec::with_capacity(runs);
    let mut details = String::new();

    for _ in 0..runs {
        let (mve, run) = measure(|| Mve::open(&path));
        let mve = mve.expect("MAXINT.MVE opens");
        if details.is_empty() {
            let info = mve.info();
            let audio = info
                .audio
                .map(|audio| {
                    format!(
                        "{} Hz, {} ch, {} bit{}",
                        audio.sample_rate,
                        audio.channels,
                        audio.bits,
                        if audio.compressed { ", delta" } else { "" }
                    )
                })
                .unwrap_or_else(|| "none".to_string());
            details = format!(
                "frames={} size={}x{} fps={:.2} formats={:?} audio={audio}",
                info.frame_count,
                info.width,
                info.height,
                info.frames_per_second(),
                info.used_video_formats(),
            );
        }
        stats.push(run);
        drop(mve);
    }

    report("mve", "Mve::open", &stats);
    println!("[mve] info: {details}");

    // First frame: decoder allocation (two frame buffers) + decompression.
    let mve = Mve::open(&path).expect("MAXINT.MVE opens");
    let mut frame_stats = Vec::with_capacity(runs);

    for _ in 0..runs {
        let (frame_len, run) = measure(|| {
            let mut decoder = mve.decoder();
            let frame = decoder.next_frame().expect("frame decodes");
            frame.map(<[u8]>::len)
        });
        let frame_len = frame_len.expect("MAXINT.MVE has a first frame");
        assert!(frame_len > 0);
        frame_stats.push(run);
    }

    report("mve", "first next_frame", &frame_stats);
}

// ---------------------------------------------------------------------------
// Entry point.
// ---------------------------------------------------------------------------

fn print_usage() {
    println!("bench_assets [--data-dir PATH] [--runs N] [--mode res|sprite|palette|msc|mve|all]");
}

fn main() {
    let mut data_dir: Option<PathBuf> = None;
    let mut runs = DEFAULT_RUNS;
    let mut mode = String::from("all");
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => {
                data_dir = Some(PathBuf::from(args.next().unwrap_or_else(|| {
                    eprintln!("--data-dir needs a path");
                    std::process::exit(2);
                })));
            }
            "--runs" => {
                runs = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .filter(|&value| value >= 1)
                    .unwrap_or_else(|| {
                        eprintln!("--runs needs a positive integer");
                        std::process::exit(2);
                    });
            }
            "--mode" => {
                mode = args.next().unwrap_or_else(|| {
                    eprintln!("--mode needs a value");
                    std::process::exit(2);
                });
            }
            "-h" | "--help" => {
                print_usage();
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                print_usage();
                std::process::exit(2);
            }
        }
    }

    let data_dir = data_dir.unwrap_or_else(|| {
        PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../max-original"
        ))
    });

    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!("asset benchmark - max_assets (phase 0 of asset-storage.md)");
    println!(
        "profile={profile} os={} arch={} runs={runs}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!("data dir: {}", data_dir.display());
    println!("peak = logical live bytes high-water mark (tracking allocator)");
    println!();

    if cfg!(debug_assertions) {
        println!("WARNING: debug build - timings are not representative of the game");
        println!();
    }

    if mode == "all" {
        for name in ["res", "sprite", "palette", "msc", "mve"] {
            run_mode(name, &data_dir, runs);
            println!();
        }
    } else {
        run_mode(&mode, &data_dir, runs);
    }
}

fn run_mode(name: &str, data_dir: &Path, runs: usize) {
    match name {
        "res" => bench_res(data_dir, runs),
        "sprite" => bench_sprite(data_dir, runs),
        "palette" => bench_palette(data_dir, runs),
        "msc" => bench_msc(data_dir, runs),
        "mve" => bench_mve(data_dir, runs),
        other => {
            eprintln!("unknown mode: {other}");
            print_usage();
            std::process::exit(2);
        }
    }
}
