//! Sentiment and emotion analysis via ONNX models.

pub mod onnx_engine;

pub use onnx_engine::OnnxSentimentEngine;

use serde::{Deserialize, Serialize};

/// A single emotion score from a multi-label model (e.g., go-emotions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmotionScore {
    pub label: String,
    pub score: f64,
}

/// Result from a sentiment analysis pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentData {
    /// Dominant label (e.g. "joy", "neutral", "positive").
    pub label: String,
    /// Confidence score [0, 1].
    pub score: f64,
    /// Top-K emotions (for multi-label models like go-emotions).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emotions: Option<Vec<EmotionScore>>,
}

/// Trait implemented by all sentiment engines.
pub trait SentimentEngine: Send + Sync {
    fn analyze(&self, text: &str) -> SentimentData;
}
