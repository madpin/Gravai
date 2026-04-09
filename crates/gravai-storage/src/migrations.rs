//! Schema migration runner.

use rusqlite::Connection;

/// All migrations in order. Each is a (version, description, sql) tuple.
const MIGRATIONS: &[(u32, &str, &str)] = &[
    (1, "Initial schema", include_str!("../sql/001_initial.sql")),
    (
        2,
        "Embeddings and chat",
        include_str!("../sql/002_embeddings.sql"),
    ),
    (
        3,
        "Sentiment columns",
        include_str!("../sql/003_sentiment.sql"),
    ),
    (
        4,
        "Chat conversations",
        include_str!("../sql/004_chat_conversations.sql"),
    ),
];

/// Run all pending migrations. Creates the schema_version table if needed.
pub fn run_migrations(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY,
            description TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;

    let current_version: u32 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )?;

    for &(version, description, sql) in MIGRATIONS {
        if version > current_version {
            tracing::info!("Applying migration v{version}: {description}");
            conn.execute_batch(sql)?;
            conn.execute(
                "INSERT INTO schema_version (version, description) VALUES (?1, ?2)",
                rusqlite::params![version, description],
            )?;
        }
    }

    Ok(())
}
