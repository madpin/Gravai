//! Performance monitoring and benchmarking utilities.

use serde::Serialize;

/// Get current process memory usage (RSS) in bytes via getrusage — sandbox-safe.
#[cfg(target_os = "macos")]
pub fn memory_usage_bytes() -> u64 {
    unsafe {
        let mut usage = std::mem::zeroed::<libc::rusage>();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        // ru_maxrss is in bytes on macOS (unlike Linux where it's KB)
        usage.ru_maxrss as u64
    }
}

#[cfg(not(target_os = "macos"))]
pub fn memory_usage_bytes() -> u64 {
    0
}

// Stores (wall_time, user_cpu_secs, sys_cpu_secs) from the last cpu_usage_pct call.
#[cfg(target_os = "macos")]
static PREV_CPU: std::sync::Mutex<Option<(std::time::Instant, f64, f64)>> =
    std::sync::Mutex::new(None);

#[cfg(target_os = "macos")]
fn timeval_secs(tv: &libc::timeval) -> f64 {
    tv.tv_sec as f64 + tv.tv_usec as f64 / 1_000_000.0
}

/// Get instantaneous CPU usage of this process as a percentage (macOS only).
/// Uses getrusage deltas between calls — sandbox-safe, no subprocess.
#[cfg(target_os = "macos")]
pub fn cpu_usage_pct() -> f64 {
    unsafe {
        let mut usage = std::mem::zeroed::<libc::rusage>();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        let now = std::time::Instant::now();
        let user = timeval_secs(&usage.ru_utime);
        let sys = timeval_secs(&usage.ru_stime);

        let mut guard = PREV_CPU.lock().unwrap();
        let result = if let Some((prev_time, prev_user, prev_sys)) = *guard {
            let wall = now.duration_since(prev_time).as_secs_f64();
            if wall > 0.05 {
                ((user - prev_user + sys - prev_sys) / wall * 100.0)
                    .max(0.0)
                    .min(100.0)
            } else {
                0.0
            }
        } else {
            0.0
        };

        *guard = Some((now, user, sys));
        result
    }
}

#[cfg(not(target_os = "macos"))]
pub fn cpu_usage_pct() -> f64 {
    0.0
}

/// Get total system memory in bytes via sysctl syscall — sandbox-safe.
#[cfg(target_os = "macos")]
pub fn total_system_memory() -> u64 {
    unsafe {
        let mut mib = [libc::CTL_HW, libc::HW_MEMSIZE];
        let mut size: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            &mut size as *mut u64 as *mut libc::c_void,
            &mut len,
            std::ptr::null_mut(),
            0,
        );
        size
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
    pub cpu_pct: f64,
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
        cpu_pct: cpu_usage_pct(),
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
