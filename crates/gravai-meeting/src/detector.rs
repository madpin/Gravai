//! Meeting app detection via process monitoring.
//!
//! Uses `ps` command to check running processes instead of ScreenCaptureKit,
//! so we don't trigger the Screen Recording permission just for detection.

use serde::Serialize;
use std::collections::HashSet;
use tracing::{debug, info};

/// Known meeting app process names and their display names.
const MEETING_PROCESSES: &[(&str, &str)] = &[
    ("zoom.us", "Zoom"),
    ("Microsoft Teams", "Microsoft Teams"),
    ("Slack", "Slack"),
    ("FaceTime", "FaceTime"),
    ("Discord", "Discord"),
    ("Webex", "WebEx"),
    ("Google Chrome", "Google Chrome"), // Could be running Meet
];

/// Known meeting app bundle IDs (used when SCK is already active during recording).
const MEETING_BUNDLE_IDS: &[(&str, &str)] = &[
    ("us.zoom.xos", "Zoom"),
    ("com.microsoft.teams2", "Microsoft Teams"),
    ("com.microsoft.teams", "Microsoft Teams"),
    ("com.tinyspeck.slackmacgap", "Slack"),
    ("com.apple.FaceTime", "FaceTime"),
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

/// Check for running meeting apps using `ps` (no Screen Recording permission needed).
pub fn detect_meeting_apps() -> Vec<DetectedMeeting> {
    let running = get_running_process_names();
    let mut detected = Vec::new();

    for (process_name, app_name) in MEETING_PROCESSES {
        if running.iter().any(|p| p.contains(process_name)) {
            // Find matching bundle ID if known
            let bundle_id = MEETING_BUNDLE_IDS
                .iter()
                .find(|(_, name)| name == app_name)
                .map(|(id, _)| id.to_string());

            detected.push(DetectedMeeting {
                app_name: app_name.to_string(),
                bundle_id,
                source: "process".into(),
            });
        }
    }

    if !detected.is_empty() {
        debug!(
            "Detected meeting apps: {:?}",
            detected.iter().map(|d| &d.app_name).collect::<Vec<_>>()
        );
    }

    detected
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

    /// Poll once and return newly detected meetings (not seen in previous poll).
    pub fn poll(&mut self) -> Vec<DetectedMeeting> {
        let current = detect_meeting_apps();
        let current_names: HashSet<String> = current.iter().map(|d| d.app_name.clone()).collect();

        let new_meetings: Vec<DetectedMeeting> = current
            .into_iter()
            .filter(|d| !self.last_detected.contains(&d.app_name))
            .collect();

        self.last_detected = current_names;
        new_meetings
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
        let new_meetings = detector.poll();
        for meeting in new_meetings {
            event_bus.publish(gravai_core::GravaiEvent::MeetingDetected {
                app_name: meeting.app_name,
                window_title: None,
            });
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
        // Should at least find some processes on any system
        assert!(!procs.is_empty());
    }

    #[test]
    fn meeting_detector_tracks_state() {
        let config = gravai_config::MeetingDetectionConfig::default();
        let mut detector = MeetingDetector::new(&config);
        let _ = detector.poll();
    }
}
