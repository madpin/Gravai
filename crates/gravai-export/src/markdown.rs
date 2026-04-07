//! Markdown export with YAML frontmatter.
//! Timestamps in transcript show elapsed time (MM:SS) from session start.

use crate::{ExportData, ExportOptions};

pub fn export_markdown(data: &ExportData, options: &ExportOptions) -> String {
    let mut md = String::new();

    // YAML frontmatter
    md.push_str("---\n");
    md.push_str(&format!(
        "title: \"{}\"\n",
        data.title.as_deref().unwrap_or(&data.session_id)
    ));
    md.push_str(&format!("date: {}\n", &data.started_at));
    if let Some(ref app) = data.meeting_app {
        md.push_str(&format!("meeting_app: {app}\n"));
    }
    if let Some(dur) = data.duration_seconds {
        md.push_str(&format!("duration_seconds: {:.0}\n", dur));
    }
    md.push_str(&format!("session_id: {}\n", data.session_id));
    md.push_str("---\n\n");

    // Title + date
    md.push_str(&format!(
        "# {}\n\n",
        data.title.as_deref().unwrap_or("Meeting Transcript")
    ));
    md.push_str(&format!("**Date:** {}\n\n", &data.started_at));

    // Summary
    if options.include_summary {
        if let Some(ref s) = data.summary {
            md.push_str("## Summary\n\n");
            md.push_str(&format!("{}\n\n", s.tldr));

            if !s.key_decisions.is_empty() {
                md.push_str("### Key Decisions\n\n");
                for d in &s.key_decisions {
                    md.push_str(&format!("- {d}\n"));
                }
                md.push('\n');
            }

            if options.include_action_items && !s.action_items.is_empty() {
                md.push_str("### Action Items\n\n");
                for a in &s.action_items {
                    let desc = a["description"].as_str().unwrap_or("");
                    let owner = a["owner"].as_str();
                    if let Some(o) = owner {
                        md.push_str(&format!("- [ ] {desc} (@{o})\n"));
                    } else {
                        md.push_str(&format!("- [ ] {desc}\n"));
                    }
                }
                md.push('\n');
            }

            if !s.open_questions.is_empty() {
                md.push_str("### Open Questions\n\n");
                for q in &s.open_questions {
                    md.push_str(&format!("- {q}\n"));
                }
                md.push('\n');
            }
        }
    }

    // Transcript — timestamps as MM:SS elapsed from session start
    if options.include_transcript && !data.utterances.is_empty() {
        md.push_str("## Transcript\n\n");

        // Use session start as the base time for relative timestamps
        let base_time = parse_timestamp(&data.started_at)
            .or_else(|| parse_timestamp(&data.utterances[0].timestamp));

        for u in &data.utterances {
            let speaker = u.speaker.as_deref().unwrap_or(&u.source);
            let elapsed = match (base_time, parse_timestamp(&u.timestamp)) {
                (Some(base), Some(ts)) => {
                    let diff_secs = ts.saturating_sub(base) / 1000;
                    let m = diff_secs / 60;
                    let s = diff_secs % 60;
                    format!("{m:02}:{s:02}")
                }
                _ => u.timestamp.clone(),
            };
            md.push_str(&format!("**[{elapsed}] {speaker}:** {}\n\n", u.text));
        }
    }

    md
}

/// Try to parse a timestamp string into milliseconds since epoch.
/// Handles ISO 8601 (2026-04-07T10:00:00Z) and HH:MM:SS formats.
fn parse_timestamp(ts: &str) -> Option<u64> {
    // Try chrono ISO parse
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        return Some(dt.timestamp_millis() as u64);
    }
    // Try "YYYY-MM-DDTHH:MM:SS" without timezone
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(dt.and_utc().timestamp_millis() as u64);
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt.and_utc().timestamp_millis() as u64);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ExportUtterance;

    #[test]
    fn export_produces_valid_markdown() {
        let data = ExportData {
            session_id: "test-001".into(),
            title: Some("Team Standup".into()),
            started_at: "2026-04-07T10:00:00Z".into(),
            ended_at: None,
            duration_seconds: Some(300.0),
            meeting_app: Some("Zoom".into()),
            utterances: vec![
                ExportUtterance {
                    timestamp: "2026-04-07T10:00:05Z".into(),
                    source: "microphone".into(),
                    speaker: Some("You".into()),
                    text: "Good morning everyone".into(),
                },
                ExportUtterance {
                    timestamp: "2026-04-07T10:01:30Z".into(),
                    source: "system_audio".into(),
                    speaker: Some("Remote".into()),
                    text: "Hey, good morning".into(),
                },
            ],
            summary: None,
        };
        let md = export_markdown(&data, &ExportOptions::default());
        assert!(md.contains("Team Standup"));
        assert!(md.contains("[00:05]")); // 5 seconds in
        assert!(md.contains("[01:30]")); // 1 min 30 sec in
        assert!(md.contains("**Date:**"));
        // Full datetime should NOT appear per line
        assert!(!md.contains("[2026-04-07"));
    }

    #[test]
    fn parse_rfc3339() {
        assert!(parse_timestamp("2026-04-07T10:00:00Z").is_some());
    }
}
