//! SQLite storage: sessions, utterances, FTS5 search, migrations.

mod database;
mod migrations;

pub use database::{Database, SessionRecord, UtteranceRecord};
