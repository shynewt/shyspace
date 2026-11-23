//! Capacity probes: they build large workspaces and print how time, disk
//! and bucket requests grow with history, members and files, so limits are
//! measured rather than guessed. They are slow, so they're ignored by
//! default; run them one at a time, with temporary files on disk:
//!
//! ```sh
//! TMPDIR=/var/tmp/shyspace-capacity nice cargo test --release -p shy-core \
//!     capacity -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `SHY_CAPACITY_SCALE` (default 1, may be a fraction) multiplies every
//! size. Each probe checks its results are right at that size; timings are
//! printed, not asserted, because they depend on the machine.

mod catch_up;
mod counting_store;
mod files;
mod history;
mod members;

use std::{
    path::Path,
    time::{Duration, Instant},
};

const READ_RUNS: usize = 3;
/// A read slower than this is timed once: repeating it only costs time.
const SLOW_READ: Duration = Duration::from_secs(1);

/// `base` times `SHY_CAPACITY_SCALE`, at least 1.
pub fn scaled(base: usize) -> usize {
    let scale: f64 = std::env::var("SHY_CAPACITY_SCALE")
        .map(|value| value.parse().expect("SHY_CAPACITY_SCALE must be a number"))
        .unwrap_or(1.0);
    assert!(scale > 0.0, "SHY_CAPACITY_SCALE must be positive");
    ((base as f64 * scale).round() as usize).max(1)
}

pub fn timed<T>(work: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let value = work();
    (value, start.elapsed())
}

/// The median of a few runs, so one hiccup doesn't skew a read's time.
pub fn median_time<T>(mut work: impl FnMut() -> T) -> Duration {
    let first = timed(&mut work).1;
    if first >= SLOW_READ {
        return first;
    }
    let mut times: Vec<Duration> = std::iter::once(first)
        .chain((1..READ_RUNS).map(|_| timed(&mut work).1))
        .collect();
    times.sort();
    times[READ_RUNS / 2]
}

pub fn ms(duration: Duration) -> String {
    format!("{:.1} ms", duration.as_secs_f64() * 1000.0)
}

pub fn per_second(count: usize, duration: Duration) -> String {
    format!("{:.0}/s", count as f64 / duration.as_secs_f64())
}

pub fn megabytes(bytes: u64) -> String {
    format!("{:.2} MB", bytes as f64 / 1_000_000.0)
}

/// The size of a member's main database, with its write-ahead log.
pub fn database_bytes(member_dir: &Path) -> u64 {
    ["shy.db", "shy.db-wal"]
        .iter()
        .filter_map(|name| std::fs::metadata(member_dir.join(name)).ok())
        .map(|metadata| metadata.len())
        .sum()
}

/// The most memory this test process has held so far (Linux only).
pub fn peak_memory() -> String {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return "unknown".into();
    };
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .map_or("unknown".into(), |value| value.trim().to_owned())
}
