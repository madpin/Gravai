//! Performance monitoring and benchmarking utilities.

use serde::Serialize;

/// Get current process memory usage (RSS) in bytes.
#[cfg(target_os = "macos")]
pub fn memory_usage_bytes() -> u64 {
    match std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
    {
        Ok(output) => {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<u64>()
                .unwrap_or(0)
                * 1024 // ps reports in KB
        }
        Err(_) => 0,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn memory_usage_bytes() -> u64 {
    0
}

/// Get total system memory in bytes.
#[cfg(target_os = "macos")]
pub fn total_system_memory() -> u64 {
    match std::process::Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
    {
        Ok(output) => String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u64>()
            .unwrap_or(0),
        Err(_) => 0,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn total_system_memory() -> u64 {
    0
}

#[derive(Debug, Serialize)]
pub struct PerfSnapshot {
    pub rss_mb: f64,
    pub total_memory_gb: f64,
    pub memory_pct: f64,
    pub uptime_seconds: f64,
    pub session_count: u32,
}

static START_TIME: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

pub fn init() {
    START_TIME.get_or_init(std::time::Instant::now);
}

pub fn snapshot(session_count: u32) -> PerfSnapshot {
    let rss = memory_usage_bytes();
    let total = total_system_memory();
    let uptime = START_TIME
        .get()
        .map(|t| t.elapsed().as_secs_f64())
        .unwrap_or(0.0);

    PerfSnapshot {
        rss_mb: rss as f64 / 1_048_576.0,
        total_memory_gb: total as f64 / 1_073_741_824.0,
        memory_pct: if total > 0 {
            (rss as f64 / total as f64) * 100.0
        } else {
            0.0
        },
        uptime_seconds: uptime,
        session_count,
    }
}

/// Log a performance warning if memory exceeds threshold.
pub fn check_memory_budget(budget_mb: f64) {
    let rss = memory_usage_bytes() as f64 / 1_048_576.0;
    if rss > budget_mb {
        tracing::warn!(
            "Memory usage ({:.0} MB) exceeds budget ({:.0} MB)",
            rss,
            budget_mb
        );
    }
}
