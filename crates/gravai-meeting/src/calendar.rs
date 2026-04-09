//! Calendar integration — reads upcoming events from macOS Calendar.
//!
//! Uses osascript (AppleScript) to query Calendar.app for upcoming events.
//! This avoids EventKit FFI complexity while providing the core functionality.

use serde::Serialize;
use tracing::{debug, warn};

/// A calendar event with relevant meeting info.
#[derive(Debug, Clone, Serialize)]
pub struct CalendarEvent {
    pub title: String,
    pub start_time: String,
    pub end_time: String,
    pub notes: Option<String>,
    pub location: Option<String>,
}

/// Query macOS Calendar for events happening now or in the next `lead_time_seconds`.
#[cfg(target_os = "macos")]
pub fn get_current_events(lead_time_seconds: u32) -> Vec<CalendarEvent> {
    let script = format!(
        r#"
        set now to current date
        set windowEnd to now + ({} * 60)
        set eventList to {{}}
        tell application "Calendar"
            repeat with cal in calendars
                -- In-progress events: started before now, not yet ended
                set inProgress to (every event of cal whose start date ≤ now and end date ≥ now)
                repeat with evt in inProgress
                    try
                        set eventTitle to summary of evt
                        set eventStart to start date of evt
                        set eventEnd to end date of evt
                        set eventNotes to ""
                        try
                            set eventNotes to description of evt
                        end try
                        set eventLocation to ""
                        try
                            set eventLocation to location of evt
                        end try
                        set end of eventList to "inprogress|" & eventTitle & "|" & (eventStart as string) & "|" & (eventEnd as string) & "|" & eventNotes & "|" & eventLocation
                    end try
                end repeat
                -- Upcoming events within the look-ahead window
                set upcoming to (every event of cal whose start date > now and start date ≤ windowEnd)
                repeat with evt in upcoming
                    try
                        set eventTitle to summary of evt
                        set eventStart to start date of evt
                        set eventEnd to end date of evt
                        set eventNotes to ""
                        try
                            set eventNotes to description of evt
                        end try
                        set eventLocation to ""
                        try
                            set eventLocation to location of evt
                        end try
                        set end of eventList to "upcoming|" & eventTitle & "|" & (eventStart as string) & "|" & (eventEnd as string) & "|" & eventNotes & "|" & eventLocation
                    end try
                end repeat
            end repeat
        end tell
        set AppleScript's text item delimiters to "||"
        return eventList as string
        "#,
        (lead_time_seconds + 300) / 60
    );

    match std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
    {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                debug!("Calendar query failed: {stderr}");
                return Vec::new();
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_calendar_output(&stdout)
        }
        Err(e) => {
            warn!("Failed to run osascript for calendar: {e}");
            Vec::new()
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn get_current_events(_lead_time_seconds: u32) -> Vec<CalendarEvent> {
    Vec::new()
}

/// Try to find a matching calendar event for session naming.
///
/// Prefers events that are currently in progress (started in the past, not yet
/// ended) over upcoming events, and further prefers events that contain a Zoom
/// link in their location or notes.
pub fn find_meeting_title(lead_time_seconds: u32) -> Option<String> {
    let events = get_current_events(lead_time_seconds);
    if events.is_empty() {
        return None;
    }

    // Prefer events with a Zoom link (most likely the active Zoom meeting)
    let zoom_event = events.iter().find(|e| {
        let loc = e.location.as_deref().unwrap_or("").to_lowercase();
        let notes = e.notes.as_deref().unwrap_or("").to_lowercase();
        loc.contains("zoom.us")
            || notes.contains("zoom.us")
            || loc.contains("zoommtg://")
            || notes.contains("zoommtg://")
    });
    if let Some(ev) = zoom_event {
        return Some(ev.title.clone());
    }

    // Fall back to the first event
    events.first().map(|e| e.title.clone())
}

fn parse_calendar_output(output: &str) -> Vec<CalendarEvent> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    // New format: "status|title|start|end|notes|location"
    // Preserve in-progress events first (they appear first in the output).
    trimmed
        .split("||")
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.split('|').collect();
            // Minimum: status|title|start|end (4 fields)
            if parts.len() >= 4 {
                let notes = parts
                    .get(4)
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(String::from);
                let location = parts
                    .get(5)
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(String::from);
                Some(CalendarEvent {
                    title: parts[1].trim().to_string(),
                    start_time: parts[2].trim().to_string(),
                    end_time: parts[3].trim().to_string(),
                    notes,
                    location,
                })
            } else if parts.len() == 3 {
                // Backwards-compatible: old format without status prefix
                Some(CalendarEvent {
                    title: parts[0].trim().to_string(),
                    start_time: parts[1].trim().to_string(),
                    end_time: parts[2].trim().to_string(),
                    notes: None,
                    location: None,
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_output() {
        assert!(parse_calendar_output("").is_empty());
        assert!(parse_calendar_output("  \n  ").is_empty());
    }

    #[test]
    fn parse_single_event() {
        let output = "Team Standup|Monday, April 7, 2026 at 10:00:00 AM|Monday, April 7, 2026 at 10:30:00 AM";
        let events = parse_calendar_output(output);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "Team Standup");
    }

    #[test]
    fn parse_multiple_events() {
        let output = "Meeting A|start1|end1||Meeting B|start2|end2";
        let events = parse_calendar_output(output);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].title, "Meeting A");
        assert_eq!(events[1].title, "Meeting B");
    }
}
