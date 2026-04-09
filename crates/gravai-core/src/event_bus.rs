//! Typed async pub/sub event bus.
//!
//! Ported from ears-rust-api event_bus.rs, upgraded from serde_json::Value
//! to a typed GravaiEvent enum.

use serde::Serialize;
use tokio::sync::broadcast;

const CHANNEL_CAPACITY: usize = 256;

/// All events that can be published through the event bus.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum GravaiEvent {
    /// Session state changed (idle, recording, paused, stopped)
    SessionStateChanged {
        state: String,
        session_id: Option<String>,
    },

    /// New utterance transcribed
    TranscriptUpdated {
        session_id: String,
        utterance_id: i64,
        source: String,
        speaker: Option<String>,
        text: String,
        timestamp: String,
    },

    /// Audio volume level update for a source
    VolumeLevel { source: String, db: f64 },

    /// Meeting app detected
    MeetingDetected {
        app_name: String,
        window_title: Option<String>,
    },

    /// Meeting app ended (was previously detected, now gone)
    MeetingEnded { app_name: String },

    /// Capture preset activated
    PresetActivated { preset_id: String },

    /// Profile switched
    ProfileSwitched { profile_id: String },

    /// Model download progress
    DownloadProgress {
        model_name: String,
        bytes_downloaded: u64,
        bytes_total: Option<u64>,
    },

    /// Generic error notification
    Error { message: String },

    /// Utterances have been corrected by the LLM post-processing pass
    TranscriptCorrected {
        session_id: String,
        utterance_ids: Vec<i64>,
    },
}

#[derive(Debug, Clone)]
pub struct EventBus {
    sender: broadcast::Sender<GravaiEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self { sender }
    }

    /// Publish an event to all subscribers.
    pub fn publish(&self, event: GravaiEvent) {
        let _ = self.sender.send(event);
    }

    /// Subscribe to the event stream.
    pub fn subscribe(&self) -> broadcast::Receiver<GravaiEvent> {
        self.sender.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_and_subscribe() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();

        bus.publish(GravaiEvent::SessionStateChanged {
            state: "recording".into(),
            session_id: Some("test-123".into()),
        });

        let event = rx.recv().await.unwrap();
        match event {
            GravaiEvent::SessionStateChanged { state, session_id } => {
                assert_eq!(state, "recording");
                assert_eq!(session_id.unwrap(), "test-123");
            }
            _ => panic!("Unexpected event type"),
        }
    }

    #[tokio::test]
    async fn no_subscribers_does_not_panic() {
        let bus = EventBus::new();
        bus.publish(GravaiEvent::Error {
            message: "test".into(),
        });
        // Should not panic even with no subscribers
    }
}
