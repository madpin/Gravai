//! Prompt templates for meeting summarization, analysis, and transcript correction.

use minijinja::Environment;
use tracing::warn;

pub const DEFAULT_SUMMARY_SYSTEM: &str = r#"You are a meeting intelligence assistant. Given a transcript, produce a structured meeting summary as a single JSON object with these fields:
- "tldr": 2-3 sentence summary (string)
- "key_decisions": array of strings (decisions made)
- "action_items": array of {"description": string, "owner": string or null}
- "open_questions": array of strings (unresolved topics)

Reply with the JSON object ONLY — start with `{` and end with `}`. No prose, no markdown fences, no commentary before or after."#;

pub const DEFAULT_SUMMARY_USER: &str = r#"Summarize this meeting transcript:

{% for u in utterances %}
[{{ u.timestamp }}] {{ u.source }}{% if u.speaker %} ({{ u.speaker }}){% endif %}: {{ u.text }}
{% endfor %}

Return the JSON object now. Empty arrays are fine when a section has nothing.
Begin your reply with `{`."#;

pub const CORRECTION_SYSTEM: &str = "You are a transcript correction assistant. \
Fix transcription errors (spelling, proper nouns, punctuation) using the provided \
knowledge base. Preserve the speaker's original meaning and phrasing. \
Return ONLY the corrected lines, one per input, keeping the [id] prefix exactly as given.";

pub const CORRECTION_USER: &str = r#"## Context
{% for entry in knowledge %}
### {{ entry.title }}
{{ entry.text }}

{% endfor %}
## Transcript to correct
{% for u in utterances %}
[{{ u.id }}] {% if u.speaker %}{{ u.speaker }}: {% endif %}{{ u.text }}
{% endfor %}

Return one corrected line per input with the exact same [id] prefix. Do not add commentary."#;

/// Render a Jinja2 template with the given context.
pub fn render_prompt(template_str: &str, context: &serde_json::Value) -> Result<String, String> {
    let mut env = Environment::new();
    env.add_template("prompt", template_str)
        .map_err(|e| format!("Template parse error: {e}"))?;
    let tmpl = env
        .get_template("prompt")
        .map_err(|e| format!("Template: {e}"))?;
    let rendered = tmpl.render(context).map_err(|e| {
        warn!("Template render error: {e}");
        format!("Template render: {e}")
    })?;
    Ok(rendered.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_simple_template() {
        let tmpl = "Hello {{ name }}!";
        let ctx = serde_json::json!({"name": "World"});
        let result = render_prompt(tmpl, &ctx).unwrap();
        assert_eq!(result, "Hello World!");
    }

    #[test]
    fn render_loop_template() {
        let tmpl = "{% for item in items %}{{ item }} {% endfor %}";
        let ctx = serde_json::json!({"items": ["a", "b", "c"]});
        let result = render_prompt(tmpl, &ctx).unwrap();
        assert_eq!(result, "a b c");
    }
}
