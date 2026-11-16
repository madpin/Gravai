CREATE TABLE session_audio_archival (
    session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    replay_path TEXT,
    mime TEXT,
    state TEXT NOT NULL,
    raw_deleted_at TEXT,
    error TEXT
);
