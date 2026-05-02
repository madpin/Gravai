---
description: Guide to gravai-intelligence crate — LLM, summarization, diarization, sentiment, embeddings, RAG chat, correction
allowed-tools: Read, Glob, Grep
---

You are helping with the `gravai-intelligence` crate at `crates/gravai-intelligence/`. Provide accurate, concise information about the requested topic.

## Crate Overview
All AI/ML features beyond transcription: LLM chat, meeting summarization, speaker diarization, sentiment analysis, vector embeddings, transcript correction, and RAG-based Q&A.

## Key Modules

### `llm_client.rs` — LLM Integration
```rust
pub enum LlmClient {
    Local { engine: Arc<LocalLlmEngine> },
    Api { base_url: String, model: String, api_key: Option<String>, client: reqwest::Client },
}
```
- `async fn new(config: &LlmConfig) -> Result<Self, String>`: Dispatches on provider
- `chat(messages, max_tokens, temperature) -> Result<String>`: Local GGUF inference or HTTP POST
- Two backends: `"local"` (mistral.rs in-process GGUF) or `"api"` (any OpenAI-compatible endpoint)
- Config: `LlmConfig { provider, local_model, base_url, model, api_key, max_tokens }` in `gravai-config`

### `chat.rs` — Ask Gravai (RAG)
```rust
pub struct ChatResponse {
    pub answer: String,
    pub citations: Vec<ChatCitation>,
}
pub struct ChatCitation {
    pub session_id: String,
    pub utterance_id: String,
    pub timestamp: DateTime<Utc>,
    pub text_snippet: String,
}
```
- `ask_gravai(question, session_id?, conversation_id?) -> Result<ChatResponse>`
- Flow: embed question → hybrid search (FTS5 + semantic) → build LLM prompt with context utterances → call LLM → extract citation references `[0]`, `[1]` → save turn in DB
- System prompt: forces answers only from transcript context
- Session-scoped: if `session_id` provided, only searches that session's utterances
- Conversation history: loads last N turns from DB for multi-turn context

### `summarization/` — Meeting Summary
```rust
pub struct MeetingSummary {
    pub tldr: String,
    pub key_decisions: Vec<String>,
    pub action_items: Vec<String>,
    pub open_questions: Vec<String>,
}
pub trait SummarizationProvider {
    async fn summarize(&self, utterances: &[UtteranceRecord]) -> Result<MeetingSummary>;
}
```
- `LlmSummarizationProvider`: formats transcript + calls LLM with Minijinja template prompt
- Template in `prompts.rs`

### `diarization/` — Speaker Identification
```rust
pub struct SpeakerSegment { pub speaker_id: String, pub start_ms: u64, pub end_ms: u64, pub confidence: f32 }
pub trait DiarizationProvider: Send {
    async fn diarize(&self, samples: &[f32]) -> Result<Vec<SpeakerSegment>>;
}
```
- `EnergyDiarizer`: simple loudness-based speaker separation (built-in, no model needed)
- `PyannoteDiarizer`: ONNX pyannote model-based (more accurate, ~model download required)
- Selected via `FeaturesConfig.diarization_engine` in config

### `sentiment/` — Emotion Classification
```rust
pub trait SentimentEngine: Send {
    async fn analyze(&self, text: &str) -> Result<Vec<EmotionScore>>;
}
pub struct EmotionScore { pub label: String, pub score: f32 }  // joy, sadness, anger, fear, etc.
pub struct SentimentData { pub dominant_emotion: String, pub scores: Vec<EmotionScore> }
```
- `OnnxSentimentEngine`: distilbert-based emotion classifier (ONNX)
- Results stored per utterance in SQLite, queryable via `get_session_sentiment()`

### `embeddings/` — Vector Embeddings
```rust
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
}
```
- `BagOfWordsEmbedder`: built-in, no model needed, low quality
- `AllMiniLmEmbedder`: ~22MB ONNX, good quality/size tradeoff
- `GemmaEmbedder`: ~274MB ONNX, high quality
- `BgeM3Embedder`: ~572MB ONNX, highest quality
- Stored in SQLite via `store_embedding()`, retrieved via `semantic_search()`

### `correction/` — Transcript Correction
- `TranscriptCorrectionProvider`: async LLM-based ASR error correction
- Triggered by batch (N utterances) + debounce (N seconds) from `CorrectionConfig`
- Uses knowledge base entries as context for domain-specific corrections
- Publishes `GravaiEvent::TranscriptCorrected` on fix

### `prompts.rs` — Minijinja Templates
- Templates for: summarization, correction, chat system prompt
- Keep prompt changes in templates, not in Rust code

## Tauri Commands
- `summarize_session(session_id)` — load transcript → summarize → return `MeetingSummary`
- `ask_gravai(question, session_id?, conversation_id?)` — RAG Q&A → `ChatResponse`
- `get_session_sentiment()` — per-speaker dominant emotion summary
- `generate_embeddings(session_id)` — embed all utterances → store in DB (required before semantic search)

## Configuration Keys
```json
{
  "llm": { "provider": "local", "local_model": "gemma3-4b-q4", "base_url": "", "model": "", "max_tokens": 2048 },
  "embedding": { "model": "all-minilm" },
  "features": {
    "diarization": true, "diarization_engine": "energy",
    "sentiment_analysis": false, "echo_suppression": true
  },
  "correction": { "enabled": false, "batch_size": 5, "debounce_seconds": 10.0 }
}
```

---

Now answer the user's question about `gravai-intelligence`: $ARGUMENTS
