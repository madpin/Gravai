//! ONNX Runtime sentiment engine — ported from ears-rust-api.
//! Supports go-emotions (28-class multi-label) which is the primary model used by Gravai.

use super::{EmotionScore, SentimentData, SentimentEngine};
use ort::session::Session;
use ort::value::Tensor;
use std::sync::Mutex;
use tokenizers::Tokenizer;

const GO_EMOTIONS_LABELS: &[&str] = &[
    "admiration",
    "amusement",
    "anger",
    "annoyance",
    "approval",
    "caring",
    "confusion",
    "curiosity",
    "desire",
    "disappointment",
    "disapproval",
    "disgust",
    "embarrassment",
    "excitement",
    "fear",
    "gratitude",
    "grief",
    "joy",
    "love",
    "nervousness",
    "optimism",
    "pride",
    "realization",
    "relief",
    "remorse",
    "sadness",
    "surprise",
    "neutral",
];

fn models_dir() -> std::path::PathBuf {
    gravai_config::models_dir().join("sentiment")
}

pub struct OnnxSentimentEngine {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    top_k: usize,
}

impl OnnxSentimentEngine {
    /// Load the go-emotions ONNX model from `~/.gravai/models/sentiment/go-emotions/`.
    /// Returns `None` if the model files are not present (graceful no-op).
    pub fn try_load() -> Option<Self> {
        let model_dir = models_dir().join("go-emotions");
        let model_path = model_dir.join("model.onnx");
        let tokenizer_path = model_dir.join("tokenizer.json");

        if !model_path.exists() || !tokenizer_path.exists() {
            tracing::debug!(
                "go-emotions model not found at {} — sentiment disabled",
                model_dir.display()
            );
            return None;
        }

        let session = match Session::builder().and_then(|mut b| b.commit_from_file(&model_path)) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to load go-emotions ONNX model: {e}");
                return None;
            }
        };

        let tokenizer = match Tokenizer::from_file(&tokenizer_path) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Failed to load go-emotions tokenizer: {e}");
                return None;
            }
        };

        tracing::info!("go-emotions sentiment engine loaded");
        Some(Self {
            session: Mutex::new(session),
            tokenizer,
            top_k: 5,
        })
    }
}

impl SentimentEngine for OnnxSentimentEngine {
    fn analyze(&self, text: &str) -> SentimentData {
        let neutral = || SentimentData {
            label: "neutral".into(),
            score: 0.0,
            emotions: None,
        };

        let encoding = match self.tokenizer.encode(text, true) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Tokenizer error: {e}");
                return neutral();
            }
        };

        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect();
        let len = input_ids.len();

        let ids_tensor = match Tensor::from_array((vec![1i64, len as i64], input_ids)) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Tensor create error: {e}");
                return neutral();
            }
        };
        let mask_tensor = match Tensor::from_array((vec![1i64, len as i64], attention_mask)) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Tensor create error: {e}");
                return neutral();
            }
        };

        let mut session = match self.session.lock() {
            Ok(s) => s,
            Err(_) => return neutral(),
        };

        let outputs = match session.run(ort::inputs![
            "input_ids" => ids_tensor,
            "attention_mask" => mask_tensor,
        ]) {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!("ONNX inference error: {e}");
                return neutral();
            }
        };

        let logits: Vec<f32> = match outputs.iter().next() {
            Some((_name, tensor)) => match tensor.try_extract_tensor::<f32>() {
                Ok((_shape, data)) => data.to_vec(),
                Err(_) => return neutral(),
            },
            None => return neutral(),
        };

        // go-emotions uses sigmoid (multi-label)
        let probs = sigmoid_vec(&logits);
        let mut indexed: Vec<(usize, f64)> = probs.iter().copied().enumerate().collect();
        indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let top_emotions: Vec<EmotionScore> = indexed
            .iter()
            .take(self.top_k)
            .map(|(idx, score)| EmotionScore {
                label: GO_EMOTIONS_LABELS
                    .get(*idx)
                    .unwrap_or(&"unknown")
                    .to_string(),
                score: round4(*score),
            })
            .collect();

        let label = top_emotions
            .first()
            .map(|e| e.label.clone())
            .unwrap_or_else(|| "neutral".into());
        let score = top_emotions.first().map(|e| e.score).unwrap_or(0.0);

        SentimentData {
            label,
            score,
            emotions: Some(top_emotions),
        }
    }
}

fn sigmoid_vec(logits: &[f32]) -> Vec<f64> {
    logits
        .iter()
        .map(|&x| 1.0 / (1.0 + (-(x as f64)).exp()))
        .collect()
}

fn round4(v: f64) -> f64 {
    (v * 10000.0).round() / 10000.0
}
