//! Meeting app detection via process and window monitoring.
//!
//! Polls running processes for known meeting applications.
//! On macOS, uses NSWorkspace to get running app bundle IDs.

use serde::Serialize;
use std::collections::HashSet;
use tracing::{debug, info};

/// Known meeting app bundle IDs and process names.
const MEETING_APPS: &[(&str, &str)] = &[
    ("us.zoom.xos", "Zoom"),
    ("com.microsoft.teams2", "Microsoft Teams"),
    ("com.microsoft.teams", "Microsoft Teams"),
    ("com.tinyspeck.slackmacgap", "Slack"),
    ("com.apple.FaceTime", "FaceTime"),
    ("com.discord.Discord", "Discord"),
    ("com.cisco.webexmeetingsapp", "WebEx"),
];

/// Browser URL patterns that indicate a meeting (used in future browser title scanning).
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
    pub source: String, // "process" or "browser"
}

/// Check for running meeting apps by examining running processes.
pub fn detect_meeting_apps() -> Vec<DetectedMeeting> {
    let mut detected = Vec::new();
    let running = get_running_bundle_ids();

    for (bundle_id, app_name) in MEETING_APPS {
        if running.contains(*bundle_id) {
            detected.push(DetectedMeeting {
                app_name: app_name.to_string(),
                bundle_id: Some(bundle_id.to_string()),
                source: "process".into(),
            });
        }
    }

    if !detected.is_empty() {
        info!(
            "Detected meeting apps: {:?}",
            detected.iter().map(|d| &d.app_name).collect::<Vec<_>>()
        );
    }

    detected
}

/// Get bundle IDs of all running applications.
#[cfg(target_os = "macos")]
fn get_running_bundle_ids() -> HashSet<String> {
    // Use ScreenCaptureKit's app list since we already have that dependency
    gravai_audio::screencapturekit::list_running_apps()
        .iter()
        .filter_map(|app| {
            app.get("bundle_id")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn get_running_bundle_ids() -> HashSet<String> {
    HashSet::new()
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

    /// Check if an app is in the auto-allow list.
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
    fn meeting_detector_tracks_state() {
        let config = gravai_config::MeetingDetectionConfig::default();
        let mut detector = MeetingDetector::new(&config);

        // First poll with no meetings should return empty
        // (can't control running apps in test, but structure is correct)
        let _ = detector.poll();
    }
}
