---
description: Guide to gravai-storage crate — SQLite schema, CRUD, FTS5 search, embeddings, sessions, utterances
allowed-tools: Read, Glob, Grep
---

You are helping with the `gravai-storage` crate at `crates/gravai-storage/`. Provide accurate, concise information about the requested topic.

## Crate Overview
SQLite-based persistence for all Gravai data: sessions, utterances, vector embeddings, RAG conversations, and the knowledge base.

## Database Location
- Production: `~/.gravai/gravai.db`
- Debug: `~/.gravai-dev/gravai.db`
- WAL mode enabled, foreign keys enabled

## `Database` struct (`database.rs`)
Central type — wraps `rusqlite::Connection`. One instance managed via `Arc<Mutex<Database>>` in `AppState`.

## Core Record Types

### `SessionRecord`
```rust
pub struct SessionRecord {
    pub id: String,              // YYYYMMDD_HHMMSS
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_seconds: Option<f64>,
    pub title: Option<String>,
    pub meeting_app: Option<String>,  // "zoom", "teams", etc.
    pub state: String,               // "recording", "stopped"
}
```

### `UtteranceRecord`
```rust
pub struct UtteranceRecord {
    pub id: String,
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
    pub source: String,          // "mic" or "system"
    pub speaker: Option<String>,  // from diarization
    pub text: String,
    pub confidence: Option<f32>,
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
    // Optional enrichments:
    pub sentiment: Option<String>,
    pub emotion_scores: Option<String>,  // JSON
    pub corrected_text: Option<String>,  // from LLM correction
}
```

### `KnowledgeEntry`
```rust
pub struct KnowledgeEntry {
    pub id: String,
    pub category: String,       // "person", "project", "term", etc.
    pub name: String,
    pub aliases: Vec<String>,   // JSON array in DB
    pub context: String,        // used in correction prompts
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

## Key Methods

### Sessions
```rust
create_session(record: &SessionRecord) -> Result<()>
get_session(id: &str) -> Result<SessionRecord>
list_sessions(limit: usize, offset: usize) -> Result<Vec<SessionRecord>>
delete_session(id: &str) -> Result<()>          // cascades to utterances
rename_session(id: &str, title: &str) -> Result<()>
update_session_state(id: &str, state: &str, ended_at: Option<DateTime<Utc>>, duration: Option<f64>) -> Result<()>
```

### Utterances
```rust
insert_utterance(record: &UtteranceRecord) -> Result<()>
get_utterances(session_id: &str) -> Result<Vec<UtteranceRecord>>
get_utterances_by_speaker(session_id: &str, speaker: &str) -> Result<Vec<UtteranceRecord>>
update_utterance_correction(id: &str, corrected: &str) -> Result<()>
```

### Search
```rust
// FTS5 full-text search across all utterances
search_utterances(query: &str, limit: usize) -> Result<Vec<UtteranceRecord>>

// Vector similarity search (cosine distance)
semantic_search(embedding: &[f32], limit: usize) -> Result<Vec<UtteranceRecord>>

// Combines FTS5 + semantic, deduplicates, ranks by relevance
hybrid_search(query: &str, embedding: &[f32], limit: usize) -> Result<Vec<UtteranceRecord>>

// Filtered session list (date range, meeting app, search query)
search_sessions_filtered(filter: SessionFilter) -> Result<Vec<SessionRecord>>
```

### Embeddings
```rust
store_embedding(utterance_id: &str, embedding: &[f32]) -> Result<()>
// Retrieved via semantic_search() — no direct getter needed
```

### Conversations (RAG Chat)
```rust
create_conversation(session_id: Option<&str>) -> Result<String>  // returns conversation_id
get_conversation(id: &str) -> Result<Vec<ConversationMessage>>
list_conversations() -> Result<Vec<ConversationMeta>>
save_message(conversation_id: &str, role: &str, content: &str, citations: Option<Vec<ChatCitation>>) -> Result<()>
```

### Knowledge Base
```rust
create_knowledge_entry(entry: &KnowledgeEntry) -> Result<()>
list_knowledge_entries() -> Result<Vec<KnowledgeEntry>>
update_knowledge_entry(entry: &KnowledgeEntry) -> Result<()>
delete_knowledge_entry(id: &str) -> Result<()>
get_active_knowledge_entries() -> Result<Vec<KnowledgeEntry>>  // for correction prompts
```

## Schema Notes
- `sessions` table: primary key is the session ID string
- `utterances` table: FK to sessions (cascade delete), FTS5 virtual table `utterances_fts` mirrors text column
- `embeddings` table: utterance_id FK, `embedding` stored as BLOB (f32 array)
- `conversations` + `conversation_messages` tables for RAG chat history
- `knowledge` table for knowledge base
- `migrations.rs` runs auto-migration on DB open (incremental version tracking)

## Tauri Commands
- `get_storage_info()` → disk usage per session (audio file sizes + utterance counts)
- `delete_session_audio(session_id)` → remove WAV files, keep DB records
- `delete_full_session(session_id)` → remove WAV + DB records
- `save_realtime_transcript(session_id, path)` → export utterances to markdown file

---

Now answer the user's question about `gravai-storage`: $ARGUMENTS
