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
                set evts to (every event of cal whose start date ≥ (now - 5 * minutes) and start date ≤ windowEnd)
                repeat with evt in evts
                    set eventTitle to summary of evt
                    set eventStart to start date of evt
                    set eventEnd to end date of evt
                    set end of eventList to eventTitle & "|" & (eventStart as string) & "|" & (eventEnd as string)
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
pub fn find_meeting_title(lead_time_seconds: u32) -> Option<String> {
    let events = get_current_events(lead_time_seconds);
    // Return the first event's title if any
    events.first().map(|e| e.title.clone())
}

fn parse_calendar_output(output: &str) -> Vec<CalendarEvent> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    trimmed
        .split("||")
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.split('|').collect();
            if parts.len() >= 3 {
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
