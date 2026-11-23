//! What each open, fully live workspace costs this device: memory, and CPU
//! while idle (#22 caps how many run live). Each workspace here starts real
//! networking, as the app does. Ignored by default:
//!
//! ```sh
//! cargo test --release -p shy-core --lib open_workspaces -- --ignored --nocapture
//! ```

use std::time::Duration;

use crate::{
    Networking, Profile,
    tests::support::*,
    workspaces::{DeviceKeys, Workspaces},
};

const WORKSPACES: usize = 8;
const IDLE: Duration = Duration::from_secs(30);

/// Resident memory, in MB.
fn resident_mb() -> f64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = status.lines().find(|line| line.starts_with("VmRSS:")).unwrap();
    let kilobytes: f64 = line.split_whitespace().nth(1).unwrap().parse().unwrap();
    kilobytes / 1024.0
}

/// CPU time this process has used, user and system, in seconds.
fn cpu_seconds() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
    let fields: Vec<&str> = stat.rsplit(')').next().unwrap().split_whitespace().collect();
    let ticks: f64 = fields[11].parse::<f64>().unwrap() + fields[12].parse::<f64>().unwrap();
    ticks / 100.0
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "slow, and starts real networking"]
async fn open_workspaces() {
    let root = tempfile::tempdir().unwrap();
    let keys = root.path().join("keys");
    std::fs::create_dir_all(&keys).unwrap();
    let bucket = memory_bucket();
    let workspaces = Workspaces::with_connector(
        root.path().join("app"),
        DeviceKeys::Files(keys),
        connector(&bucket),
    );
    let start = resident_mb();
    let mut open = Vec::new();
    for number in 1..=WORKSPACES {
        let profile = Profile {
            name: "Ada",
            is_bot: false,
        };
        let workspace = workspaces
            .create(
                &format!("Workspace {number}"),
                profile,
                test_bucket_config(),
                Networking::Online,
            )
            .await
            .unwrap();
        open.push(workspace);
        println!(
            "{number} open: {:.0} MB resident (+{:.1} MB)",
            resident_mb(),
            resident_mb() - start
        );
    }
    tokio::time::sleep(Duration::from_secs(5)).await;
    let cpu_before = cpu_seconds();
    tokio::time::sleep(IDLE).await;
    let idle_cpu = cpu_seconds() - cpu_before;
    println!(
        "{WORKSPACES} open and idle for {}s: {:.2}s CPU ({:.2}% of one core), {:.0} MB resident",
        IDLE.as_secs(),
        idle_cpu,
        idle_cpu / IDLE.as_secs_f64() * 100.0,
        resident_mb()
    );
}
