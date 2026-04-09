-- Knowledge base for transcript correction: user-curated names, projects, jargon, etc.
CREATE TABLE IF NOT EXISTS knowledge_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    category TEXT NOT NULL DEFAULT 'other',   -- 'person','project','company','place','jargon','style','other'
    name TEXT NOT NULL,                        -- canonical form, e.g. "João Pinto"
    aliases TEXT,                              -- JSON array of likely ASR misspellings
    context TEXT,                              -- free-text hint: "CTO of Acme Corp"
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_knowledge_category ON knowledge_entries(category);
CREATE INDEX IF NOT EXISTS idx_knowledge_active ON knowledge_entries(active);
