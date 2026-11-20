//! SQLite storage: connection setup, append-only migrations and data-access helpers.
//!
//! No business rules here (spec §5, §3.6). The database lives at
//! `<app data dir>/gabaritei.sqlite` (see [`database_path`]).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{AppError, AppResult};

/// File name of the database inside the app data directory.
pub const FILE_NAME: &str = "gabaritei.sqlite";

/// How long a statement waits for a lock held by another connection.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Schema migrations, applied in order. `PRAGMA user_version` stores how many have run.
///
/// Append-only: never edit or reorder a released migration; add a new file instead.
const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_initial.sql")];

/// Schema version this build of the app expects.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// Message shown when the database was created by a newer version of the app.
pub const NEWER_SCHEMA_MESSAGE: &str = "Estes dados foram criados por uma versão mais recente do Gabaritei. \
     Atualize o aplicativo para abri-los.";

/// Path of the database file inside the OS app data directory.
pub fn database_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(FILE_NAME)
}

/// The app's database connection, shared by every command.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Opens (or creates) the database file, configures it and applies pending migrations.
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(path)?;
        configure(&conn)?;
        migrate(&mut conn, MIGRATIONS)?;
        Ok(Self::from_connection(conn))
    }

    /// Opens a private in-memory database with the full schema (for tests).
    pub fn open_in_memory() -> AppResult<Self> {
        let mut conn = Connection::open_in_memory()?;
        configure(&conn)?;
        migrate(&mut conn, MIGRATIONS)?;
        Ok(Self::from_connection(conn))
    }

    fn from_connection(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }

    /// Runs `f` with exclusive access to the connection.
    pub fn with<T>(&self, f: impl FnOnce(&mut Connection) -> AppResult<T>) -> AppResult<T> {
        let mut conn = self.lock()?;
        f(&mut conn)
    }

    /// Runs `f` inside a transaction: committed if `f` returns `Ok`, rolled back otherwise.
    pub fn transaction<T>(&self, f: impl FnOnce(&Transaction<'_>) -> AppResult<T>) -> AppResult<T> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    fn lock(&self) -> AppResult<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| AppError::Internal("database mutex poisoned".into()))
    }
}

/// Connection-level settings. WAL only applies to file databases; in-memory ones keep `memory`.
fn configure(conn: &Connection) -> AppResult<()> {
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    let _mode: String =
        conn.pragma_update_and_check(None, "journal_mode", "wal", |row| row.get(0))?;
    Ok(())
}

/// Brings the schema up to `migrations.len()`. Each migration runs in its own transaction
/// together with the `user_version` bump, so a failure leaves the previous version intact.
fn migrate(conn: &mut Connection, migrations: &[&str]) -> AppResult<()> {
    let current = user_version(conn)?;
    let target = migrations.len() as u32;
    if current > target {
        return Err(AppError::Invalid(NEWER_SCHEMA_MESSAGE.into()));
    }
    for (index, sql) in migrations.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", index as u32 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

/// Schema version recorded in the database file.
pub fn user_version(conn: &Connection) -> AppResult<u32> {
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

/// Reads a JSON setting. `Ok(None)` when the key was never set.
pub fn get_setting<T: DeserializeOwned>(conn: &Connection, key: &str) -> AppResult<Option<T>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?;
    raw.map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(Into::into)
}

/// Stores a setting as JSON, replacing any previous value.
pub fn set_setting<T: Serialize>(conn: &Connection, key: &str, value: &T) -> AppResult<()> {
    let json = serde_json::to_string(value)?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, json],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TABLES: &[&str] = &[
        "attempts",
        "collection_items",
        "collections",
        "drafts",
        "key_changes",
        "question_topics",
        "questions",
        "responses",
        "review_state",
        "settings",
        "topics",
    ];

    fn tables(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    /// Every schema object's SQL, to compare the schema before and after reopening.
    fn schema_sql(conn: &Connection) -> Vec<(String, Option<String>)> {
        let mut stmt = conn
            .prepare("SELECT name, sql FROM sqlite_schema ORDER BY type, name")
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn insert_question(conn: &Connection, id: &str) -> rusqlite::Result<usize> {
        conn.execute(
            "INSERT INTO questions (id, type, stem, options, answer_id, created_at, updated_at)
             VALUES (?1, 'boolean', 'Enunciado', '[{\"id\":\"c\",\"text\":\"Certo\"},{\"id\":\"e\",\"text\":\"Errado\"}]',
                     'c', '2026-11-19T00:00:00Z', '2026-11-19T00:00:00Z')",
            params![id],
        )
    }

    #[test]
    fn database_lives_in_the_app_data_dir() {
        let dir = Path::new("/data/io.github.tiagoguerreirodev.gabaritei");
        assert_eq!(database_path(dir), dir.join("gabaritei.sqlite"));
    }

    #[test]
    fn fresh_database_has_every_table_and_the_latest_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = database_path(&dir.path().join("nested").join("app-data"));
        let db = Database::open(&path).unwrap();

        assert!(path.exists(), "open creates the folder and the file");
        db.with(|conn| {
            assert_eq!(tables(conn), TABLES);
            assert_eq!(user_version(conn)?, SCHEMA_VERSION);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn reopening_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = database_path(dir.path());

        let first = Database::open(&path).unwrap();
        let before = first.with(|conn| Ok(schema_sql(conn))).unwrap();
        first
            .with(|conn| set_setting(conn, "probe", &json!({ "kept": true })))
            .unwrap();
        drop(first);

        let second = Database::open(&path).unwrap();
        second
            .with(|conn| {
                assert_eq!(schema_sql(conn), before);
                assert_eq!(user_version(conn)?, SCHEMA_VERSION);
                assert_eq!(
                    get_setting::<serde_json::Value>(conn, "probe")?,
                    Some(json!({ "kept": true }))
                );
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn newer_database_is_refused_with_a_pt_br_message_and_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = database_path(dir.path());
        drop(Database::open(&path).unwrap());
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
                .unwrap();
        }

        let error = Database::open(&path)
            .err()
            .expect("newer schema is refused");
        assert!(matches!(&error, AppError::Invalid(_)));
        assert_eq!(error.to_string(), NEWER_SCHEMA_MESSAGE);

        let conn = Connection::open(&path).unwrap();
        assert_eq!(user_version(&conn).unwrap(), SCHEMA_VERSION + 1);
    }

    #[test]
    fn connection_uses_wal_foreign_keys_and_a_busy_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&database_path(dir.path())).unwrap();
        db.with(|conn| {
            let mode: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
            let fk: i64 = conn.pragma_query_value(None, "foreign_keys", |r| r.get(0))?;
            let timeout: i64 = conn.pragma_query_value(None, "busy_timeout", |r| r.get(0))?;
            assert_eq!(mode, "wal");
            assert_eq!(fk, 1);
            assert_eq!(timeout, BUSY_TIMEOUT.as_millis() as i64);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn foreign_key_violations_are_rejected() {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            let orphan_topic_link = conn.execute(
                "INSERT INTO question_topics (question_id, topic_id) VALUES ('missing', 1)",
                [],
            );
            assert!(orphan_topic_link.is_err());

            conn.execute(
                "INSERT INTO collections (id, title, kind, created_at)
                 VALUES ('c1', 'Caderno', 'imported', '2026-11-19T00:00:00Z')",
                [],
            )?;
            let orphan_item = conn.execute(
                "INSERT INTO collection_items (collection_id, position, question_id)
                 VALUES ('c1', 0, 'missing')",
                [],
            );
            let message = orphan_item.unwrap_err().to_string();
            assert!(message.contains("FOREIGN KEY"), "{message}");

            insert_question(conn, "q1")?;
            conn.execute(
                "INSERT INTO collection_items (collection_id, position, question_id)
                 VALUES ('c1', 0, 'q1')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn check_constraints_reject_invalid_values() {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            let bad_type = conn.execute(
                "INSERT INTO questions (id, type, stem, options, answer_id, created_at, updated_at)
                 VALUES ('q', 'essay', 'x', '[]', '', 't', 't')",
                [],
            );
            assert!(bad_type.is_err(), "essays are out of scope");

            let bad_json = conn.execute(
                "INSERT INTO settings (key, value) VALUES ('k', 'not json')",
                [],
            );
            assert!(bad_json.is_err());

            let blank_topic = conn.execute(
                "INSERT INTO topics (area, matter, topic) VALUES ('Direito', '  ', 'Crase')",
                [],
            );
            assert!(blank_topic.is_err());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn failed_migration_rolls_back_and_keeps_the_version() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        let migrations = [
            "CREATE TABLE a (x INTEGER) STRICT;",
            "CREATE TABLE b (x INTEGER) STRICT; THIS IS NOT SQL;",
        ];

        assert!(migrate(&mut conn, &migrations).is_err());
        assert_eq!(user_version(&conn).unwrap(), 1);
        assert_eq!(tables(&conn), ["a"]);
    }

    #[test]
    fn transaction_commits_on_ok_and_rolls_back_on_error() {
        let db = Database::open_in_memory().unwrap();
        db.transaction(|tx| set_setting(tx, "kept", &1)).unwrap();
        let failed: AppResult<()> = db.transaction(|tx| {
            set_setting(tx, "dropped", &2)?;
            Err(AppError::Invalid("falha".into()))
        });
        assert!(failed.is_err());

        db.with(|conn| {
            assert_eq!(get_setting::<i64>(conn, "kept")?, Some(1));
            assert_eq!(get_setting::<i64>(conn, "dropped")?, None);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn settings_round_trip_as_json() {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            assert_eq!(get_setting::<String>(conn, "missing")?, None);
            set_setting(
                conn,
                "role",
                &json!({ "agent": "claude", "effort": "high" }),
            )?;
            set_setting(conn, "role", &json!({ "agent": "codex", "effort": "low" }))?;
            assert_eq!(
                get_setting::<serde_json::Value>(conn, "role")?,
                Some(json!({ "agent": "codex", "effort": "low" }))
            );
            Ok(())
        })
        .unwrap();
    }
}
