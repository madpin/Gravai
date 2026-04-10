//! Calendar integration — reads upcoming events from macOS Calendar.
//!
//! Primary: uses EventKit via objc2 bindings (instant, uses native indexes).
//! Fallback: osascript (slow with large synced calendars, 60s timeout).

use serde::Serialize;
use tracing::{debug, info, warn};

/// A calendar event with relevant meeting info.
#[derive(Debug, Clone, Serialize)]
pub struct CalendarEvent {
    pub title: String,
    pub start_time: String,
    pub end_time: String,
    pub notes: Option<String>,
    pub location: Option<String>,
    /// Whether the event is currently in progress.
    pub in_progress: bool,
}

/// Query macOS Calendar for events happening now or in the next `lead_time_seconds`.
#[cfg(target_os = "macos")]
pub fn get_current_events(lead_time_seconds: u32) -> Vec<CalendarEvent> {
    // Try native EventKit first (instant)
    match get_events_via_eventkit(lead_time_seconds) {
        Ok(events) => {
            debug!("EventKit returned {} events", events.len());
            return events;
        }
        Err(e) => {
            info!("EventKit unavailable ({e}), falling back to osascript");
        }
    }

    // Fallback: osascript (slow but works via Calendar.app's own TCC access)
    get_events_via_osascript(lead_time_seconds)
}

#[cfg(not(target_os = "macos"))]
pub fn get_current_events(_lead_time_seconds: u32) -> Vec<CalendarEvent> {
    Vec::new()
}

// ── EventKit (native, fast) ─────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn get_events_via_eventkit(lead_time_seconds: u32) -> Result<Vec<CalendarEvent>, String> {
    use objc2::AllocAnyThread;
    use objc2_event_kit::{EKAuthorizationStatus, EKEntityType, EKEventStore};
    use objc2_foundation::{NSArray, NSDate, NSString};

    unsafe {
        let store = EKEventStore::init(EKEventStore::alloc());

        // Check authorization — if not fullAccess, return Err to trigger fallback
        let status = EKEventStore::authorizationStatusForEntityType(EKEntityType::Event);
        if status != EKAuthorizationStatus::FullAccess {
            return Err(format!("Calendar access: {:?} (need FullAccess)", status.0));
        }

        let now = NSDate::now();
        let look_back_secs = 4.0 * 3600.0; // 4 hours
        let look_ahead_secs = (lead_time_seconds + 300) as f64;
        let start = NSDate::dateWithTimeIntervalSinceNow(-look_back_secs);
        let end = NSDate::dateWithTimeIntervalSinceNow(look_ahead_secs);

        let predicate = store.predicateForEventsWithStartDate_endDate_calendars(&start, &end, None);

        let ek_events = store.eventsMatchingPredicate(&predicate);

        let now_ti = now.timeIntervalSince1970();
        let mut events = Vec::new();

        let count = NSArray::count(&ek_events);
        for i in 0..count {
            let ek_event = NSArray::objectAtIndex(&ek_events, i);

            // Skip all-day events (holidays, OOO, etc.) — not real meetings
            if ek_event.isAllDay() {
                continue;
            }

            // Skip events that have already ended
            let end_date = ek_event.endDate();
            if end_date.timeIntervalSince1970() < now_ti {
                continue;
            }

            let title = ek_event.title().to_string();
            let start_date = ek_event.startDate();
            let start_ti = start_date.timeIntervalSince1970();
            let end_ti = end_date.timeIntervalSince1970();
            let notes: Option<String> = ek_event
                .notes()
                .map(|s: objc2::rc::Retained<NSString>| s.to_string());
            let location: Option<String> = ek_event
                .location()
                .map(|s: objc2::rc::Retained<NSString>| s.to_string());
            let in_progress = start_ti <= now_ti;

            if !title.is_empty() {
                events.push(CalendarEvent {
                    title,
                    start_time: format!("{start_ti:.0}"),
                    end_time: format!("{end_ti:.0}"),
                    notes,
                    location,
                    in_progress,
                });
            }
        }

        // Sort: in-progress first, then by start time
        events.sort_by(|a, b| {
            b.in_progress
                .cmp(&a.in_progress)
                .then(a.start_time.cmp(&b.start_time))
        });

        Ok(events)
    }
}

// ── osascript fallback (slow) ───────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn get_events_via_osascript(lead_time_seconds: u32) -> Vec<CalendarEvent> {
    let look_ahead_min = (lead_time_seconds + 300) / 60;

    let script = format!(
        r#"
        set now to current date
        set windowStart to now - (4 * hours)
        set windowEnd to now + ({look_ahead_min} * minutes)
        set eventList to {{}}
        tell application "Calendar"
            repeat with cal in calendars
                set matchingEvents to (every event of cal whose start date ≥ windowStart and start date ≤ windowEnd)
                repeat with evt in matchingEvents
                    try
                        set eventTitle to summary of evt
                        set eventStart to start date of evt
                        set eventEnd to end date of evt
                        if eventEnd ≥ now then
                            set evtStatus to "upcoming"
                            if eventStart ≤ now then set evtStatus to "inprogress"
                            set eventNotes to ""
                            try
                                set eventNotes to description of evt
                            end try
                            set eventLocation to ""
                            try
                                set eventLocation to location of evt
                            end try
                            set end of eventList to evtStatus & "|" & eventTitle & "|" & (eventStart as string) & "|" & (eventEnd as string) & "|" & eventNotes & "|" & eventLocation
                        end if
                    end try
                end repeat
            end repeat
        end tell
        set AppleScript's text item delimiters to "||"
        return eventList as string
        "#
    );

    let mut child = match std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to spawn osascript for calendar: {e}");
            return Vec::new();
        }
    };

    // Hard timeout — 60s, always runs in background thread
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    debug!("Calendar query exited with {status}");
                    return Vec::new();
                }
                let stdout = child
                    .stdout
                    .take()
                    .map(|mut s| {
                        let mut buf = String::new();
                        std::io::Read::read_to_string(&mut s, &mut buf).ok();
                        buf
                    })
                    .unwrap_or_default();
                return parse_osascript_output(&stdout);
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    warn!("Calendar osascript timed out after 60s — killing");
                    let _ = child.kill();
                    let _ = child.wait();
                    return Vec::new();
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                warn!("Calendar query wait error: {e}");
                let _ = child.kill();
                return Vec::new();
            }
        }
    }
}

/// Try to find a matching calendar event for session naming.
///
/// Prefers events that are currently in progress over upcoming events,
/// and further prefers events that contain a meeting link (Zoom, Teams, Meet).
pub fn find_meeting_title(lead_time_seconds: u32) -> Option<String> {
    let events = get_current_events(lead_time_seconds);
    if events.is_empty() {
        return None;
    }

    // Prefer in-progress events with a meeting link
    let meeting_event = events.iter().find(|e| {
        if !e.in_progress {
            return false;
        }
        let loc = e.location.as_deref().unwrap_or("").to_lowercase();
        let notes = e.notes.as_deref().unwrap_or("").to_lowercase();
        loc.contains("zoom.us")
            || notes.contains("zoom.us")
            || loc.contains("zoommtg://")
            || notes.contains("zoommtg://")
            || loc.contains("teams.microsoft")
            || notes.contains("teams.microsoft")
            || loc.contains("meet.google")
            || notes.contains("meet.google")
    });
    if let Some(ev) = meeting_event {
        return Some(ev.title.clone());
    }

    // Any in-progress event
    if let Some(ev) = events.iter().find(|e| e.in_progress) {
        return Some(ev.title.clone());
    }

    // Fall back to the first upcoming event
    events.first().map(|e| e.title.clone())
}

fn parse_osascript_output(output: &str) -> Vec<CalendarEvent> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    trimmed
        .split("||")
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.split('|').collect();
            if parts.len() >= 4 {
                let in_progress = parts[0].trim() == "inprogress";
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
                    in_progress,
                })
            } else if parts.len() == 3 {
                Some(CalendarEvent {
                    title: parts[0].trim().to_string(),
                    start_time: parts[1].trim().to_string(),
                    end_time: parts[2].trim().to_string(),
                    notes: None,
                    location: None,
                    in_progress: false,
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
        assert!(parse_osascript_output("").is_empty());
        assert!(parse_osascript_output("  \n  ").is_empty());
    }

    #[test]
    fn parse_single_event() {
        let output = "Team Standup|Monday, April 7, 2026 at 10:00:00 AM|Monday, April 7, 2026 at 10:30:00 AM";
        let events = parse_osascript_output(output);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "Team Standup");
    }

    #[test]
    fn parse_multiple_events() {
        let output = "Meeting A|start1|end1||Meeting B|start2|end2";
        let events = parse_osascript_output(output);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].title, "Meeting A");
        assert_eq!(events[1].title, "Meeting B");
    }
}
