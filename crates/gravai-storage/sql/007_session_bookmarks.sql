-- Session bookmarks: user-created flags anchored to a point in the recording timeline.
CREATE TABLE IF NOT EXISTS session_bookmarks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    offset_ms INTEGER NOT NULL,
    note TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_bookmarks_session ON session_bookmarks(session_id, offset_ms);
