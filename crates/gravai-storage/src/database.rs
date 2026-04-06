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
            "SELECT id, session_id, timestamp, source, speaker, text, confidence, start_ms, end_ms
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
            })
        })?;
        rows.collect()
    }
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
