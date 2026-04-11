//! Session state machine and audio processing orchestrator.
//!
//! Ported from ears-rust-api session.rs — same AtomicU8 FSM pattern.
//! Manages AudioCaptureManager, per-source recording, and VAD pipeline.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};

use tokio::sync::Mutex;

/// Session states matching the ears FSM: Idle → Recording ↔ Paused → Stopped
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SessionState {
    Idle = 0,
    Recording = 1,
    Paused = 2,
    Stopped = 3,
}

impl SessionState {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Idle,
            1 => Self::Recording,
            2 => Self::Paused,
            3 => Self::Stopped,
            _ => Self::Idle,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Recording => "recording",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
        }
    }
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A recording session. Holds atomic state, config snapshot, and task handles.
pub struct Session {
    pub id: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    state: AtomicU8,
    pub config: gravai_config::AppConfig,
    pub session_dir: PathBuf,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    pub recorded_files: Mutex<Vec<PathBuf>>,
}

impl Session {
    pub fn new(id: String, config: gravai_config::AppConfig) -> Self {
        let session_dir = gravai_config::sessions_dir().join(&id);
        Self {
            id,
            started_at: chrono::Utc::now(),
            state: AtomicU8::new(SessionState::Idle as u8),
            config,
            session_dir,
            tasks: Mutex::new(Vec::new()),
            recorded_files: Mutex::new(Vec::new()),
        }
    }

    pub fn state(&self) -> SessionState {
        SessionState::from_u8(self.state.load(Ordering::SeqCst))
    }

    pub fn set_state(&self, state: SessionState) {
        self.state.store(state as u8, Ordering::SeqCst);
    }

    pub fn is_active(&self) -> bool {
        matches!(self.state(), SessionState::Recording | SessionState::Paused)
    }

    /// Register a background task handle.
    pub async fn add_task(&self, handle: tokio::task::JoinHandle<()>) {
        self.tasks.lock().await.push(handle);
    }

    /// Abort all background tasks and wait for them to actually complete.
    /// This ensures any Drop implementations (e.g. WAV finalization) have run
    /// before returning — critical for correct stop-session ordering.
    pub async fn abort_tasks(&self) {
        let handles: Vec<tokio::task::JoinHandle<()>> = {
            let mut tasks = self.tasks.lock().await;
            tasks.drain(..).collect()
        };
        for handle in handles {
            handle.abort();
            let _ = handle.await; // wait for cancellation; ignore cancelled/panic errors
        }
    }

    /// Record the duration in seconds since session started.
    pub fn duration_seconds(&self) -> f64 {
        let now = chrono::Utc::now();
        (now - self.started_at).num_milliseconds() as f64 / 1000.0
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        tracing::debug!("Session {} dropped", self.id);
    }
}

/// Generate a session ID from the current timestamp.
pub fn generate_session_id() -> String {
    chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_state_transitions() {
        let config = gravai_config::AppConfig::default();
        let session = Session::new("test-001".into(), config);
        assert_eq!(session.state(), SessionState::Idle);
        assert!(!session.is_active());

        session.set_state(SessionState::Recording);
        assert_eq!(session.state(), SessionState::Recording);
        assert!(session.is_active());

        session.set_state(SessionState::Paused);
        assert_eq!(session.state(), SessionState::Paused);
        assert!(session.is_active());

        session.set_state(SessionState::Stopped);
        assert_eq!(session.state(), SessionState::Stopped);
        assert!(!session.is_active());
    }

    #[test]
    fn state_display() {
        assert_eq!(SessionState::Recording.as_str(), "recording");
        assert_eq!(SessionState::Idle.to_string(), "idle");
    }

    #[test]
    fn generate_id_format() {
        let id = generate_session_id();
        assert_eq!(id.len(), 15); // YYYYMMDD_HHMMSS
        assert!(id.contains('_'));
    }
}
