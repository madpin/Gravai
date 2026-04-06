-- Initial Gravai schema

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    duration_seconds REAL,
    title TEXT,
    meeting_app TEXT,
    state TEXT NOT NULL DEFAULT 'idle',
    metadata TEXT  -- JSON blob for extensible fields
);

CREATE TABLE IF NOT EXISTS utterances (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    timestamp TEXT NOT NULL,
    source TEXT NOT NULL,
    speaker TEXT,
    text TEXT NOT NULL,
    confidence REAL,
    start_ms INTEGER,
    end_ms INTEGER
);

CREATE INDEX IF NOT EXISTS idx_utterances_session ON utterances(session_id);
CREATE INDEX IF NOT EXISTS idx_utterances_timestamp ON utterances(session_id, timestamp);

-- Full-text search on utterance text
CREATE VIRTUAL TABLE IF NOT EXISTS utterances_fts USING fts5(
    text,
    content=utterances,
    content_rowid=id
);

-- Triggers to keep FTS index in sync
CREATE TRIGGER IF NOT EXISTS utterances_ai AFTER INSERT ON utterances BEGIN
    INSERT INTO utterances_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TRIGGER IF NOT EXISTS utterances_ad AFTER DELETE ON utterances BEGIN
    INSERT INTO utterances_fts(utterances_fts, rowid, text) VALUES('delete', old.id, old.text);
END;

CREATE TRIGGER IF NOT EXISTS utterances_au AFTER UPDATE ON utterances BEGIN
    INSERT INTO utterances_fts(utterances_fts, rowid, text) VALUES('delete', old.id, old.text);
    INSERT INTO utterances_fts(rowid, text) VALUES (new.id, new.text);
END;

-- Session summaries (populated by AI pipeline in Phase 3)
CREATE TABLE IF NOT EXISTS session_summaries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    tldr TEXT,
    key_decisions TEXT,  -- JSON array
    action_items TEXT,   -- JSON array
    open_questions TEXT,  -- JSON array
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    provider TEXT
);

CREATE INDEX IF NOT EXISTS idx_summaries_session ON session_summaries(session_id);
