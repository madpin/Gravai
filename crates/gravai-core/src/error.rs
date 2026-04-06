//! Unified error hierarchy for Gravai.
//!
//! Transport-agnostic (no HTTP status codes). Errors propagate up to
//! Tauri commands which serialize them as JSON error responses.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum GravaiError {
    #[error("Audio error: {0}")]
    Audio(String),

    #[error("Transcription error: {0}")]
    Transcription(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Model error: {0}")]
    Model(String),

    #[error("Permission error: {0}")]
    Permission(String),

    #[error("Provider error: {0}")]
    Provider(String),

    #[error("Session error: {0}")]
    Session(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

// Convenient conversions

impl From<std::io::Error> for GravaiError {
    fn from(e: std::io::Error) -> Self {
        GravaiError::Internal(e.to_string())
    }
}

impl From<serde_json::Error> for GravaiError {
    fn from(e: serde_json::Error) -> Self {
        GravaiError::Config(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, GravaiError>;

// Make GravaiError serializable for Tauri command returns
impl serde::Serialize for GravaiError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
