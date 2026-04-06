//! Echo suppression via fuzzy string matching.
//!
//! Ported from ears-rust-api session.rs EchoSuppressor.
//! Prevents the same speech captured by both mic and system audio from
//! appearing as duplicate utterances.

use strsim::sorensen_dice;

/// Suppresses mic echoes of system audio (and vice versa) using
/// Sørensen-Dice string similarity within a sliding time window.
pub struct EchoSuppressor {
    threshold: f64,
    window_seconds: f64,
    recent: Vec<(String, f64, String)>, // (text, timestamp, source)
}

impl EchoSuppressor {
    pub fn new(threshold: f64) -> Self {
        Self {
            threshold,
            window_seconds: 30.0,
            recent: Vec::new(),
        }
    }

    /// Record a new utterance.
    pub fn add(&mut self, text: &str, source: &str) {
        let now = now_secs();
        self.prune(now);
        self.recent
            .push((text.trim().to_lowercase(), now, source.to_string()));
    }

    /// Returns true if `text` from `source` is likely an echo of a recent
    /// utterance from a *different* source.
    pub fn is_echo(&mut self, text: &str, source: &str) -> bool {
        let norm = text.trim().to_lowercase();
        if norm.is_empty() {
            return false;
        }
        let now = now_secs();
        self.prune(now);
        for (entry_text, _, entry_source) in &self.recent {
            if entry_source == source {
                continue;
            }
            let similarity = sorensen_dice(&norm, entry_text);
            if similarity >= self.threshold {
                return true;
            }
        }
        false
    }

    fn prune(&mut self, now: f64) {
        let cutoff = now - self.window_seconds;
        self.recent.retain(|(_, ts, _)| *ts >= cutoff);
    }
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_detected() {
        let mut es = EchoSuppressor::new(0.55);
        es.add("Hello how are you doing today", "system_audio");
        assert!(es.is_echo("Hello how are you doing today", "microphone"));
    }

    #[test]
    fn same_source_not_echo() {
        let mut es = EchoSuppressor::new(0.55);
        es.add("Hello how are you", "microphone");
        assert!(!es.is_echo("Hello how are you", "microphone"));
    }

    #[test]
    fn different_text_not_echo() {
        let mut es = EchoSuppressor::new(0.55);
        es.add("Hello how are you doing today", "system_audio");
        assert!(!es.is_echo("The weather is nice outside", "microphone"));
    }
}
