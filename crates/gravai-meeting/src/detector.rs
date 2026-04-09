//! Meeting app detection via process monitoring + network port checks.
//!
//! Uses `ps` command to check running processes (no Screen Recording permission needed),
//! then confirms an *active* meeting by verifying UDP network connections.
//!
//! Background: many meeting apps (Zoom, Teams) run as background processes for
//! notifications even when not in a call.  They only open UDP connections to their
//! media servers once a meeting actually starts, so counting UDP sockets is the
//! most reliable cross-platform indicator of an active call.

use serde::Serialize;
use std::collections::HashSet;
use tracing::{debug, info};

/// Describes how to confirm a process is actually in an active meeting.
#[derive(Clone, Copy)]
enum ConfirmStrategy {
    /// Process name alone is sufficient (e.g. CptHost only spawns during Zoom calls).
    ProcessOnly,
    /// Run `lsof -i 4UDP | grep <pattern>` and require count > 1.
    UdpConnections(&'static str),
}

struct MeetingProcess {
    /// Pattern matched against `ps` output.
    process: &'static str,
    /// Human-readable app name shown in the UI.
    app_name: &'static str,
    /// How to confirm the process represents an active meeting.
    confirm: ConfirmStrategy,
}

/// Known meeting app process names and their display names.
const MEETING_PROCESSES: &[MeetingProcess] = &[
    // zoom.us runs in the background for notifications; UDP > 1 = active meeting.
    MeetingProcess {
        process: "zoom.us",
        app_name: "Zoom",
        confirm: ConfirmStrategy::UdpConnections("zoom"),
    },
    // CptHost is Zoom's dedicated call-host process — only spawns during a call.
    MeetingProcess {
        process: "CptHost",
        app_name: "Zoom",
        confirm: ConfirmStrategy::ProcessOnly,
    },
    // Teams also idles in background; UDP confirms an active call.
    MeetingProcess {
        process: "Microsoft Teams",
        app_name: "Microsoft Teams",
        confirm: ConfirmStrategy::UdpConnections("Teams"),
    },
    // Slack & Discord helper renderers can be any tab, but audio call activity
    // causes a burst of UDP traffic, so UDP > 1 is a reasonable gate.
    MeetingProcess {
        process: "Slack Helper (Renderer)",
        app_name: "Slack Huddle",
        confirm: ConfirmStrategy::UdpConnections("Slack"),
    },
    MeetingProcess {
        process: "Discord Helper (Renderer)",
        app_name: "Discord",
        confirm: ConfirmStrategy::UdpConnections("Discord"),
    },
    // Webex keeps a process open; UDP gate avoids false positives.
    MeetingProcess {
        process: "Webex",
        app_name: "WebEx",
        confirm: ConfirmStrategy::UdpConnections("Webex"),
    },
    // FaceTime runs as a background app; UDP confirms an active call.
    // callservicesd is the macOS call daemon — it opens UDP during any live call,
    // so we match either process name in the lsof output.
    MeetingProcess {
        process: "FaceTime",
        app_name: "FaceTime",
        confirm: ConfirmStrategy::UdpConnections("FaceTime|callservicesd"),
    },
];

/// Known meeting app bundle IDs (used when SCK is already active during recording).
const MEETING_BUNDLE_IDS: &[(&str, &str)] = &[
    ("us.zoom.xos", "Zoom"),
    ("com.microsoft.teams2", "Microsoft Teams"),
    ("com.microsoft.teams", "Microsoft Teams"),
    ("com.tinyspeck.slackmacgap", "Slack"),
    ("com.apple.facetime", "FaceTime"),
    ("com.discord.Discord", "Discord"),
    ("com.cisco.webexmeetingsapp", "WebEx"),
];

/// Browser URL patterns that indicate a meeting (for future browser title scanning).
#[allow(dead_code)]
const MEETING_URL_PATTERNS: &[(&str, &str)] = &[
    ("meet.google.com", "Google Meet"),
    ("teams.microsoft.com", "Microsoft Teams"),
    ("zoom.us/j/", "Zoom"),
    ("zoom.us/wc/", "Zoom"),
];

/// A detected meeting app.
#[derive(Debug, Clone, Serialize)]
pub struct DetectedMeeting {
    pub app_name: String,
    pub bundle_id: Option<String>,
    pub source: String,
}

/// Check for active meeting apps.
///
/// Two-stage detection:
/// 1. Process check (`ps`) — fast, no permissions required.
/// 2. Network confirmation (`lsof`) — verifies an active call for apps that
///    idle in the background.
pub fn detect_meeting_apps() -> Vec<DetectedMeeting> {
    let running = get_running_process_names();
    let mut detected: Vec<DetectedMeeting> = Vec::new();
    let mut seen_apps: HashSet<String> = HashSet::new();

    for mp in MEETING_PROCESSES {
        if !running.iter().any(|p| p.contains(mp.process)) {
            continue;
        }

        // Skip duplicate app names (e.g. both zoom.us and CptHost detected).
        if seen_apps.contains(mp.app_name) {
            continue;
        }

        let active = match mp.confirm {
            ConfirmStrategy::ProcessOnly => true,
            ConfirmStrategy::UdpConnections(grep_pattern) => {
                is_in_active_meeting_via_udp(grep_pattern)
            }
        };

        if !active {
            debug!(
                "{} process found but no active UDP connections — skipping",
                mp.app_name
            );
            continue;
        }

        let bundle_id = MEETING_BUNDLE_IDS
            .iter()
            .find(|(_, name)| *name == mp.app_name)
            .map(|(id, _)| id.to_string());

        seen_apps.insert(mp.app_name.to_string());
        detected.push(DetectedMeeting {
            app_name: mp.app_name.to_string(),
            bundle_id,
            source: match mp.confirm {
                ConfirmStrategy::ProcessOnly => "process".into(),
                ConfirmStrategy::UdpConnections(_) => "process+udp".into(),
            },
        });
    }

    if !detected.is_empty() {
        debug!(
            "Detected active meetings: {:?}",
            detected.iter().map(|d| &d.app_name).collect::<Vec<_>>()
        );
    }

    detected
}

/// Capture the title of the active Zoom meeting window via AppleScript.
///
/// Queries `System Events` for the `zoom.us` process windows and returns the
/// first window title that is non-empty and does not start with "zoom" (which
/// filters out the base app window and lobby screen).
///
/// Requires Accessibility permission. Returns `None` silently on any failure.
#[cfg(target_os = "macos")]
pub fn get_zoom_window_title() -> Option<String> {
    let script = r#"
        tell application "System Events"
            if exists process "zoom.us" then
                tell process "zoom.us"
                    set allWindows to every window
                    repeat with w in allWindows
                        try
                            set wTitle to name of w
                            if wTitle is missing value then
                                -- skip inaccessible windows
                            else if wTitle is "" then
                                -- skip untitled windows
                            else if wTitle is "Zoom" then
                                -- skip the base app launcher window
                            else
                                return wTitle
                            end if
                        end try
                    end repeat
                end tell
            end if
        end tell
        return ""
    "#;
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let title = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // Guard against AppleScript returning the literal string "missing value"
    if title.is_empty() || title == "missing value" {
        None
    } else {
        Some(title)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn get_zoom_window_title() -> Option<String> {
    None
}

/// Returns true if `grep_pattern` has more than one UDP socket open.
///
/// Logic (from community research):
///   0 results  → app not running
///   1 result   → app open but **not** in a meeting (idle listener)
///   > 1 result → **active meeting** (media/SRTP UDP streams open)
///
/// Uses `lsof -i 4UDP` (IPv4 UDP only) to avoid false positives from
/// IPv6 or TCP connections.
fn is_in_active_meeting_via_udp(grep_pattern: &str) -> bool {
    // lsof -i 4UDP lists all IPv4 UDP sockets; we grep for the app's process name.
    let lsof = std::process::Command::new("lsof")
        .args(["-i", "4UDP", "-n", "-P"])
        .output();

    match lsof {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // grep_pattern may be "A|B" — split on '|' and match any term.
            let terms: Vec<String> = grep_pattern
                .split('|')
                .map(|t| t.trim().to_lowercase())
                .collect();
            let count = stdout
                .lines()
                .filter(|l| {
                    let lower = l.to_lowercase();
                    terms.iter().any(|t| lower.contains(t.as_str()))
                })
                .count();
            debug!("UDP check for '{}': {count} socket(s)", grep_pattern);
            count > 1
        }
        Err(e) => {
            debug!("lsof unavailable ({e}), falling back to process-only detection");
            // If lsof fails (e.g. permissions, missing tool), be optimistic and
            // treat the process as an active meeting rather than silently missing it.
            true
        }
    }
}

/// Get names of running processes using `ps` command (no SCK needed).
fn get_running_process_names() -> Vec<String> {
    match std::process::Command::new("ps")
        .args(["-eo", "comm="])
        .output()
    {
        Ok(output) => String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// A background meeting detector that polls periodically.
pub struct MeetingDetector {
    poll_interval: std::time::Duration,
    allowed_apps: HashSet<String>,
    last_detected: HashSet<String>,
}

impl MeetingDetector {
    pub fn new(config: &gravai_config::MeetingDetectionConfig) -> Self {
        Self {
            poll_interval: std::time::Duration::from_secs(5),
            allowed_apps: config.allowed_apps.iter().cloned().collect(),
            last_detected: HashSet::new(),
        }
    }

    /// Poll once.
    /// Returns `(new_meetings, ended_app_names)` — meetings newly detected
    /// this tick, and app names that were active last tick but are gone now.
    pub fn poll(&mut self) -> (Vec<DetectedMeeting>, Vec<String>) {
        let current = detect_meeting_apps();
        let current_names: HashSet<String> = current.iter().map(|d| d.app_name.clone()).collect();

        let new_meetings: Vec<DetectedMeeting> = current
            .into_iter()
            .filter(|d| !self.last_detected.contains(&d.app_name))
            .collect();

        let ended: Vec<String> = self
            .last_detected
            .iter()
            .filter(|n| !current_names.contains(*n))
            .cloned()
            .collect();

        self.last_detected = current_names;
        (new_meetings, ended)
    }

    pub fn is_auto_allowed(&self, app_name: &str) -> bool {
        self.allowed_apps.contains(app_name)
    }

    pub fn poll_interval(&self) -> std::time::Duration {
        self.poll_interval
    }
}

/// Run the meeting detection loop, emitting events when meetings are detected.
pub async fn run_detection_loop(
    config: gravai_config::MeetingDetectionConfig,
    event_bus: gravai_core::EventBus,
    active: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    if !config.enabled {
        debug!("Meeting detection disabled");
        return;
    }

    let mut detector = MeetingDetector::new(&config);
    info!(
        "Meeting detection started (poll interval: {:?})",
        detector.poll_interval()
    );

    while active.load(std::sync::atomic::Ordering::Relaxed) {
        let (new_meetings, ended) = detector.poll();
        for meeting in new_meetings {
            let window_title = if meeting.app_name == "Zoom" {
                get_zoom_window_title()
            } else {
                None
            };
            event_bus.publish(gravai_core::GravaiEvent::MeetingDetected {
                app_name: meeting.app_name,
                window_title,
            });
        }
        for app_name in ended {
            info!("Meeting ended: {app_name}");
            event_bus.publish(gravai_core::GravaiEvent::MeetingEnded { app_name });
        }
        tokio::time::sleep(detector.poll_interval()).await;
    }

    debug!("Meeting detection loop ended");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_running_processes_returns_something() {
        let procs = get_running_process_names();
        assert!(!procs.is_empty());
    }

    #[test]
    fn meeting_detector_tracks_state() {
        let config = gravai_config::MeetingDetectionConfig::default();
        let mut detector = MeetingDetector::new(&config);
        let _ = detector.poll();
    }

    #[test]
    fn udp_check_does_not_panic() {
        // Just verify lsof runs without panicking; result depends on system state.
        let _ = is_in_active_meeting_via_udp("nonexistent_app_xyz");
    }
}
