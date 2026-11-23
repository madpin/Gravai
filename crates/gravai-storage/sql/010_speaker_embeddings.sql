-- Voice vectors for remote speakers. `locked` is set when the user renames
-- that speaker, so a later clustering pass leaves the name alone.

CREATE TABLE IF NOT EXISTS utterance_speaker_embeddings (
    utterance_id INTEGER PRIMARY KEY REFERENCES utterances(id) ON DELETE CASCADE,
    embedding BLOB NOT NULL,
    locked INTEGER NOT NULL DEFAULT 0
);
