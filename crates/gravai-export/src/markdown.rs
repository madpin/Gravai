//! Markdown export with YAML frontmatter.

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

    // Title
    md.push_str(&format!(
        "# {}\n\n",
        data.title.as_deref().unwrap_or("Meeting Transcript")
    ));

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

    // Transcript
    if options.include_transcript && !data.utterances.is_empty() {
        md.push_str("## Transcript\n\n");
        for u in &data.utterances {
            let speaker = u.speaker.as_deref().unwrap_or(&u.source);
            md.push_str(&format!(
                "**[{}] {}:** {}\n\n",
                u.timestamp, speaker, u.text
            ));
        }
    }

    md
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
            utterances: vec![ExportUtterance {
                timestamp: "10:00:05".into(),
                source: "microphone".into(),
                speaker: Some("You".into()),
                text: "Good morning everyone".into(),
            }],
            summary: None,
        };
        let md = export_markdown(&data, &ExportOptions::default());
        assert!(md.contains("Team Standup"));
        assert!(md.contains("Good morning everyone"));
        assert!(md.contains("---")); // frontmatter
    }
}
