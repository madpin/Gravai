//! AI intelligence: summarization, diarization, embeddings, LLM client, chat, sentiment, correction.

pub mod chat;
pub mod correction;
pub mod diarization;
pub mod embeddings;
pub mod llm_client;
pub mod local_engine;
pub mod prompts;
pub mod sentiment;
pub mod summarization;

pub use correction::TranscriptCorrectionProvider;
pub use diarization::{DiarizationProvider, SpeakerSegment};
pub use embeddings::EmbeddingProvider;
pub use llm_client::LlmClient;
pub use sentiment::{EmotionScore, OnnxSentimentEngine, SentimentData, SentimentEngine};
pub use summarization::{MeetingSummary, SummarizationProvider};
