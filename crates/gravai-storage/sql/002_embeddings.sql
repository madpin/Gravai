-- Embeddings table for semantic search (Phase 4)
CREATE TABLE IF NOT EXISTS embeddings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    utterance_id INTEGER NOT NULL REFERENCES utterances(id) ON DELETE CASCADE,
    session_id TEXT NOT NULL,
    vector BLOB NOT NULL,
    dimension INTEGER NOT NULL DEFAULT 384,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_embeddings_session ON embeddings(session_id);
CREATE INDEX IF NOT EXISTS idx_embeddings_utterance ON embeddings(utterance_id);

-- Chat messages for Ask Gravai (Phase 4)
CREATE TABLE IF NOT EXISTS chat_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT,  -- NULL for cross-archive chat
    role TEXT NOT NULL,  -- 'user' or 'assistant'
    content TEXT NOT NULL,
    citations TEXT,  -- JSON array of {session_id, utterance_id, timestamp}
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
