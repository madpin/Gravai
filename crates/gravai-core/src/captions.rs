//! Bounded, session-scoped finalized captions. No persistent copy of live text.
use serde::Serialize;

#[derive(Debug, Default, Clone, Serialize)]
pub struct CaptionBuffer {
    pub session_id: Option<String>,
    pub state: String,
    pub lines: Vec<String>,
}
impl CaptionBuffer {
    pub fn session(&mut self, id: Option<&str>, state: &str) {
        if self.session_id.as_deref() != id || !matches!(state, "recording" | "paused") {
            self.lines.clear();
        }
        self.session_id = id.map(str::to_string);
        self.state = state.into();
    }
    pub fn push(&mut self, id: &str, text: &str) {
        if self.session_id.as_deref() != Some(id)
            || !matches!(self.state.as_str(), "recording" | "paused")
        {
            return;
        }
        self.lines.push(text.chars().take(2000).collect());
        if self.lines.len() > 8 {
            self.lines.remove(0);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundaries_clear_text_and_reject_late_results() {
        let mut b = CaptionBuffer::default();
        b.session(Some("a"), "recording");
        for _ in 0..20 {
            b.push("a", "speech");
        }
        assert_eq!(b.lines.len(), 8);
        b.session(Some("a"), "paused");
        assert_eq!(b.lines.len(), 8);
        b.session(Some("a"), "stopped");
        b.push("a", "late");
        assert!(b.lines.is_empty());
        b.session(Some("b"), "recording");
        b.push("a", "old");
        assert!(b.lines.is_empty());
    }
}
