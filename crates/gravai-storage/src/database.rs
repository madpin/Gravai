//! Database connection and CRUD operations.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::migrations;

/// A session record from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_seconds: Option<f64>,
    pub title: Option<String>,
    pub meeting_app: Option<String>,
    pub state: String,
}

/// An utterance record from the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtteranceRecord {
    pub id: i64,
    pub session_id: String,
    pub timestamp: String,
    pub source: String,
    pub speaker: Option<String>,
    pub text: String,
    pub confidence: Option<f64>,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sentiment_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sentiment_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emotions_json: Option<String>,
}

/// Main database handle.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (or create) the database at the given path, running migrations.
    pub fn open(path: &std::path::Path) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        migrations::run_migrations(&conn)?;
        Ok(Self { conn })
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        migrations::run_migrations(&conn)?;
        Ok(Self { conn })
    }

    // -- Sessions --

    pub fn create_session(&self, session: &SessionRecord) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO sessions (id, started_at, ended_at, duration_seconds, title, meeting_app, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                session.id,
                session.started_at,
                session.ended_at,
                session.duration_seconds,
                session.title,
                session.meeting_app,
                session.state,
            ],
        )?;
        Ok(())
    }

    pub fn update_session_state(
        &self,
        session_id: &str,
        state: &str,
        ended_at: Option<&str>,
        duration_seconds: Option<f64>,
    ) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "UPDATE sessions SET state = ?1, ended_at = ?2, duration_seconds = ?3 WHERE id = ?4",
            params![state, ended_at, duration_seconds, session_id],
        )?;
        Ok(())
    }

    pub fn get_session(&self, session_id: &str) -> Result<Option<SessionRecord>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, started_at, ended_at, duration_seconds, title, meeting_app, state
             FROM sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![session_id], |row| {
            Ok(SessionRecord {
                id: row.get(0)?,
                started_at: row.get(1)?,
                ended_at: row.get(2)?,
                duration_seconds: row.get(3)?,
                title: row.get(4)?,
                meeting_app: row.get(5)?,
                state: row.get(6)?,
            })
        })?;
        match rows.next() {
            Some(Ok(record)) => Ok(Some(record)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionRecord>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, started_at, ended_at, duration_seconds, title, meeting_app, state
             FROM sessions ORDER BY started_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SessionRecord {
                id: row.get(0)?,
                started_at: row.get(1)?,
                ended_at: row.get(2)?,
                duration_seconds: row.get(3)?,
                title: row.get(4)?,
                meeting_app: row.get(5)?,
                state: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    pub fn delete_session(&self, session_id: &str) -> Result<bool, rusqlite::Error> {
        let count = self
            .conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![session_id])?;
        Ok(count > 0)
    }

    // -- Utterances --

    pub fn insert_utterance(&self, u: &UtteranceRecord) -> Result<i64, rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO utterances (session_id, timestamp, source, speaker, text, confidence, start_ms, end_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                u.session_id,
                u.timestamp,
                u.source,
                u.speaker,
                u.text,
                u.confidence,
                u.start_ms,
                u.end_ms,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_utterances(
        &self,
        session_id: &str,
    ) -> Result<Vec<UtteranceRecord>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, timestamp, source, speaker, text, confidence, start_ms, end_ms,
                    sentiment_label, sentiment_score, emotions_json
             FROM utterances WHERE session_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(UtteranceRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                timestamp: row.get(2)?,
                source: row.get(3)?,
                speaker: row.get(4)?,
                text: row.get(5)?,
                confidence: row.get(6)?,
                start_ms: row.get(7)?,
                end_ms: row.get(8)?,
                sentiment_label: row.get(9)?,
                sentiment_score: row.get(10)?,
                emotions_json: row.get(11)?,
            })
        })?;
        rows.collect()
    }

    /// Full-text search across all utterances.
    pub fn search_utterances(&self, query: &str) -> Result<Vec<UtteranceRecord>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT u.id, u.session_id, u.timestamp, u.source, u.speaker, u.text, u.confidence, u.start_ms, u.end_ms
             FROM utterances u
             JOIN utterances_fts fts ON u.id = fts.rowid
             WHERE utterances_fts MATCH ?1
             ORDER BY rank",
        )?;
        let rows = stmt.query_map(params![query], |row| {
            Ok(UtteranceRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                timestamp: row.get(2)?,
                source: row.get(3)?,
                speaker: row.get(4)?,
                text: row.get(5)?,
                confidence: row.get(6)?,
                start_ms: row.get(7)?,
                end_ms: row.get(8)?,
                sentiment_label: None,
                sentiment_score: None,
                emotions_json: None,
            })
        })?;
        rows.collect()
    }

    /// Update sentiment fields on an existing utterance (by row id).
    pub fn update_utterance_sentiment(
        &self,
        id: i64,
        label: &str,
        score: f64,
        emotions_json: Option<&str>,
    ) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "UPDATE utterances SET sentiment_label = ?1, sentiment_score = ?2, emotions_json = ?3 WHERE id = ?4",
            params![label, score, emotions_json, id],
        )?;
        Ok(())
    }

    /// Fetch utterances with sentiment data for a session (only system-audio utterances with sentiment).
    pub fn get_session_sentiment(
        &self,
        session_id: &str,
    ) -> Result<Vec<UtteranceRecord>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, timestamp, source, speaker, text, confidence, start_ms, end_ms,
                    sentiment_label, sentiment_score, emotions_json
             FROM utterances
             WHERE session_id = ?1 AND source = 'system' AND sentiment_label IS NOT NULL
             ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(UtteranceRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                timestamp: row.get(2)?,
                source: row.get(3)?,
                speaker: row.get(4)?,
                text: row.get(5)?,
                confidence: row.get(6)?,
                start_ms: row.get(7)?,
                end_ms: row.get(8)?,
                sentiment_label: row.get(9)?,
                sentiment_score: row.get(10)?,
                emotions_json: row.get(11)?,
            })
        })?;
        rows.collect()
    }

    // -- Embeddings --

    pub fn store_embedding(
        &self,
        utterance_id: i64,
        session_id: &str,
        vector: &[f32],
    ) -> Result<(), rusqlite::Error> {
        let blob: Vec<u8> = vector.iter().flat_map(|f| f.to_le_bytes()).collect();
        self.conn.execute(
            "INSERT INTO embeddings (utterance_id, session_id, vector, dimension) VALUES (?1, ?2, ?3, ?4)",
            params![utterance_id, session_id, blob, vector.len() as i32],
        )?;
        Ok(())
    }

    /// Find utterances by cosine similarity to a query vector. Returns (utterance, score).
    pub fn semantic_search(
        &self,
        query_vec: &[f32],
        limit: usize,
    ) -> Result<Vec<(UtteranceRecord, f64)>, rusqlite::Error> {
        // Load all embeddings and compute cosine similarity in Rust
        // (SQLite doesn't have native vector operations)
        let mut stmt = self.conn.prepare(
            "SELECT e.utterance_id, e.vector, u.id, u.session_id, u.timestamp, u.source, u.speaker, u.text, u.confidence, u.start_ms, u.end_ms
             FROM embeddings e JOIN utterances u ON e.utterance_id = u.id",
        )?;
        let mut results: Vec<(UtteranceRecord, f64)> = stmt
            .query_map([], |row| {
                let blob: Vec<u8> = row.get(1)?;
                let record = UtteranceRecord {
                    id: row.get(2)?,
                    session_id: row.get(3)?,
                    timestamp: row.get(4)?,
                    source: row.get(5)?,
                    speaker: row.get(6)?,
                    text: row.get(7)?,
                    confidence: row.get(8)?,
                    start_ms: row.get(9)?,
                    end_ms: row.get(10)?,
                    sentiment_label: None,
                    sentiment_score: None,
                    emotions_json: None,
                };
                Ok((record, blob))
            })?
            .filter_map(|r| r.ok())
            .filter_map(|(record, blob)| {
                let stored: Vec<f32> = blob
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                let score = cosine_similarity(query_vec, &stored);
                if score > 0.0 {
                    Some((record, score))
                } else {
                    None
                }
            })
            .collect();

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        Ok(results)
    }

    // -- Advanced search with filters --

    pub fn search_sessions_filtered(
        &self,
        _query: Option<&str>,
        date_from: Option<&str>,
        date_to: Option<&str>,
        meeting_app: Option<&str>,
    ) -> Result<Vec<SessionRecord>, rusqlite::Error> {
        let mut sql = String::from(
            "SELECT id, started_at, ended_at, duration_seconds, title, meeting_app, state FROM sessions WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(from) = date_from {
            sql.push_str(" AND started_at >= ?");
            param_values.push(Box::new(from.to_string()));
        }
        if let Some(to) = date_to {
            sql.push_str(" AND started_at <= ?");
            param_values.push(Box::new(to.to_string()));
        }
        if let Some(app) = meeting_app {
            sql.push_str(" AND meeting_app = ?");
            param_values.push(Box::new(app.to_string()));
        }
        sql.push_str(" ORDER BY started_at DESC");

        let params: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok(SessionRecord {
                id: row.get(0)?,
                started_at: row.get(1)?,
                ended_at: row.get(2)?,
                duration_seconds: row.get(3)?,
                title: row.get(4)?,
                meeting_app: row.get(5)?,
                state: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    // -- Chat messages --

    pub fn save_chat_message(
        &self,
        session_id: Option<&str>,
        role: &str,
        content: &str,
        citations: Option<&str>,
    ) -> Result<i64, rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO chat_messages (session_id, role, content, citations) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, role, content, citations],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_chat_history(
        &self,
        session_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>, rusqlite::Error> {
        let (sql, param): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(sid) =
            session_id
        {
            ("SELECT role, content, citations, created_at FROM chat_messages WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2",
             vec![Box::new(sid.to_string()), Box::new(limit as i64)])
        } else {
            ("SELECT role, content, citations, created_at FROM chat_messages WHERE session_id IS NULL ORDER BY id DESC LIMIT ?1",
             vec![Box::new(limit as i64)])
        };
        let params: Vec<&dyn rusqlite::types::ToSql> = param.iter().map(|p| p.as_ref()).collect();
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params.as_slice(), |row| {
            let citations_str: Option<String> = row.get(2)?;
            Ok(serde_json::json!({
                "role": row.get::<_, String>(0)?,
                "content": row.get::<_, String>(1)?,
                "citations": citations_str.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()),
                "created_at": row.get::<_, String>(3)?,
            }))
        })?;
        let mut msgs: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
        msgs.reverse();
        Ok(msgs)
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b).map(|(x, y)| *x as f64 * *y as f64).sum();
    let mag_a: f64 = a
        .iter()
        .map(|x| (*x as f64) * (*x as f64))
        .sum::<f64>()
        .sqrt();
    let mag_b: f64 = b
        .iter()
        .map(|x| (*x as f64) * (*x as f64))
        .sum::<f64>()
        .sqrt();
    if mag_a < 1e-10 || mag_b < 1e-10 {
        return 0.0;
    }
    dot / (mag_a * mag_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_query_session() {
        let db = Database::open_in_memory().unwrap();

        let session = SessionRecord {
            id: "test-001".into(),
            started_at: "2026-04-06T12:00:00Z".into(),
            ended_at: None,
            duration_seconds: None,
            title: Some("Test Meeting".into()),
            meeting_app: Some("zoom".into()),
            state: "recording".into(),
        };
        db.create_session(&session).unwrap();

        let fetched = db.get_session("test-001").unwrap().unwrap();
        assert_eq!(fetched.title.unwrap(), "Test Meeting");
        assert_eq!(fetched.state, "recording");

        let all = db.list_sessions().unwrap();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn insert_and_search_utterances() {
        let db = Database::open_in_memory().unwrap();

        db.create_session(&SessionRecord {
            id: "s1".into(),
            started_at: "2026-04-06T12:00:00Z".into(),
            ended_at: None,
            duration_seconds: None,
            title: None,
            meeting_app: None,
            state: "recording".into(),
        })
        .unwrap();

        let id = db
            .insert_utterance(&UtteranceRecord {
                id: 0, // auto-assigned
                session_id: "s1".into(),
                timestamp: "2026-04-06T12:00:05Z".into(),
                source: "microphone".into(),
                speaker: Some("Thiago".into()),
                text: "Let's discuss the AWS migration plan".into(),
                confidence: Some(0.95),
                start_ms: Some(5000),
                end_ms: Some(8000),
            })
            .unwrap();
        assert!(id > 0);

        let utterances = db.get_utterances("s1").unwrap();
        assert_eq!(utterances.len(), 1);
        assert_eq!(utterances[0].text, "Let's discuss the AWS migration plan");

        let results = db.search_utterances("AWS migration").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].session_id, "s1");
    }

    #[test]
    fn delete_session_cascades() {
        let db = Database::open_in_memory().unwrap();

        db.create_session(&SessionRecord {
            id: "s2".into(),
            started_at: "2026-04-06T12:00:00Z".into(),
            ended_at: None,
            duration_seconds: None,
            title: None,
            meeting_app: None,
            state: "stopped".into(),
        })
        .unwrap();

        db.insert_utterance(&UtteranceRecord {
            id: 0,
            session_id: "s2".into(),
            timestamp: "2026-04-06T12:00:01Z".into(),
            source: "mic".into(),
            speaker: None,
            text: "test utterance".into(),
            confidence: None,
            start_ms: None,
            end_ms: None,
        })
        .unwrap();

        assert!(db.delete_session("s2").unwrap());
        assert!(db.get_session("s2").unwrap().is_none());
        assert!(db.get_utterances("s2").unwrap().is_empty());
    }
}
