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
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Schema migrations, applied in order. `PRAGMA user_version` stores how many have run.
///
/// Append-only: never edit or reorder a released migration; add a new file instead.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_initial.sql"),
    include_str!("../migrations/0002_tags_on_collections_and_image_alt.sql"),
];

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
        Self::from_connection(Connection::open(path)?)
    }

    /// Opens a private in-memory database with the full schema (for tests).
    pub fn open_in_memory() -> AppResult<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut conn: Connection) -> AppResult<Self> {
        prepare_connection(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
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

/// Applies connection settings and migrations when opening or replacing the database.
pub(crate) fn prepare_connection(conn: &mut Connection) -> AppResult<()> {
    configure(conn)?;
    migrate(conn, MIGRATIONS)
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

/// Stable keys in the settings table. Their stored spellings also apply to older backups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKey {
    Appearance,
    KeyboardShortcuts,
    BackupFolder,
    BackupLeftBehind,
    AiRoles,
    CliPaths,
    RemNote,
    LegacyImport,
}

crate::enum_text::stored_enum!(SettingKey {
    Appearance => "appearance",
    KeyboardShortcuts => "keyboardShortcuts",
    BackupFolder => "backup.folder",
    BackupLeftBehind => "backup.leftBehind",
    AiRoles => "ai.roles",
    CliPaths => "ai.cliPaths",
    RemNote => "ai.remnote",
    LegacyImport => "legacyImport",
});

/// Reads a JSON setting. `Ok(None)` when the key was never set.
pub fn get_setting<T: DeserializeOwned>(
    conn: &Connection,
    key: SettingKey,
) -> AppResult<Option<T>> {
    get_raw_setting(conn, key)?
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(Into::into)
}

/// A setting's stored JSON text, as is.
pub fn get_raw_setting(conn: &Connection, key: SettingKey) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?)
}

/// Writes a setting's JSON text as is, or removes the setting when `None`.
pub fn put_raw_setting(conn: &Connection, key: SettingKey, value: Option<&str>) -> AppResult<()> {
    match value {
        Some(value) => {
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }
        None => {
            conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
        }
    }
    Ok(())
}

/// Stores a setting as JSON, replacing any previous value.
pub fn set_setting<T: Serialize>(conn: &Connection, key: SettingKey, value: &T) -> AppResult<()> {
    put_raw_setting(conn, key, Some(&serde_json::to_string(value)?))
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
             VALUES (?1, ?2, 'Enunciado', '[{\"id\":\"c\",\"text\":\"Certo\"},{\"id\":\"e\",\"text\":\"Errado\"}]',
                     'c', '2026-11-19T00:00:00Z', '2026-11-19T00:00:00Z')",
            params![id, crate::bank::QuestionType::Boolean],
        )
    }

    #[test]
    fn database_lives_in_the_app_data_dir() {
        let dir = Path::new("/data/io.github.tiagoguerreirodev.gabaritei");
        assert_eq!(database_path(dir), dir.join("gabaritei.sqlite"));
    }

    #[test]
    fn opening_creates_the_nested_database_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = database_path(&dir.path().join("nested").join("app-data"));
        let _db = Database::open(&path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn a_fresh_database_contains_the_full_schema() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&database_path(dir.path())).unwrap();
        let schema = db
            .with(|conn| Ok((tables(conn), user_version(conn)?)))
            .unwrap();
        assert_eq!(
            schema,
            (
                TABLES.iter().map(|name| name.to_string()).collect(),
                SCHEMA_VERSION
            )
        );
    }

    #[test]
    fn reopening_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = database_path(dir.path());

        let first = Database::open(&path).unwrap();
        let before = first.with(|conn| Ok(schema_sql(conn))).unwrap();
        first
            .with(|conn| set_setting(conn, SettingKey::Appearance, &json!({ "kept": true })))
            .unwrap();
        drop(first);

        let second = Database::open(&path).unwrap();
        second
            .with(|conn| {
                assert_eq!(schema_sql(conn), before);
                assert_eq!(user_version(conn)?, SCHEMA_VERSION);
                assert_eq!(
                    get_setting::<serde_json::Value>(conn, SettingKey::Appearance)?,
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
    fn a_file_database_uses_wal() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&database_path(dir.path())).unwrap();
        let mode: String = db
            .with(|conn| Ok(conn.pragma_query_value(None, "journal_mode", |r| r.get(0))?))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn a_file_database_has_foreign_keys_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&database_path(dir.path())).unwrap();
        let enabled: bool = db
            .with(|conn| Ok(conn.pragma_query_value(None, "foreign_keys", |r| r.get(0))?))
            .unwrap();
        assert!(enabled);
    }

    #[test]
    fn a_file_database_configures_the_busy_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&database_path(dir.path())).unwrap();
        let timeout: u32 = db
            .with(|conn| Ok(conn.pragma_query_value(None, "busy_timeout", |r| r.get(0))?))
            .unwrap();
        assert_eq!(Duration::from_millis(timeout.into()), BUSY_TIMEOUT);
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
                 VALUES ('c1', 'Caderno', ?1, '2026-11-19T00:00:00Z')",
                [crate::bank::catalog::CollectionKind::Imported],
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

    fn columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
            .unwrap();
        stmt.query_map([table], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn upgrade_from_version_1_keeps_questions_and_moves_tags_to_collections() {
        let mut conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        migrate(&mut conn, &MIGRATIONS[..1]).unwrap();
        insert_question(&conn, "q1").unwrap();
        conn.execute("UPDATE questions SET contest = 'ALEPA' WHERE id = 'q1'", [])
            .unwrap();

        migrate(&mut conn, MIGRATIONS).unwrap();

        assert_eq!(user_version(&conn).unwrap(), SCHEMA_VERSION);
        let question_columns = columns(&conn, "questions");
        assert!(!question_columns.contains(&"contest".to_owned()));
        assert!(!question_columns.contains(&"board".to_owned()));
        assert!(question_columns.contains(&"image_alt".to_owned()));
        assert!(columns(&conn, "collections").contains(&"contest".to_owned()));
        let stem: String = conn
            .query_row("SELECT stem FROM questions WHERE id = 'q1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(stem, "Enunciado");
    }

    #[test]
    fn a_successful_transaction_commits() {
        let db = Database::open_in_memory().unwrap();
        db.transaction(|tx| set_setting(tx, SettingKey::Appearance, &1))
            .unwrap();
        assert_eq!(
            db.with(|conn| get_setting::<i64>(conn, SettingKey::Appearance))
                .unwrap(),
            Some(1)
        );
    }

    #[test]
    fn a_failed_transaction_rolls_back() {
        let db = Database::open_in_memory().unwrap();
        let failed: AppResult<()> = db.transaction(|tx| {
            set_setting(tx, SettingKey::KeyboardShortcuts, &2)?;
            Err(AppError::Invalid("falha".into()))
        });
        assert!(failed.is_err());
        assert_eq!(
            db.with(|conn| get_setting::<i64>(conn, SettingKey::KeyboardShortcuts))
                .unwrap(),
            None
        );
    }

    #[test]
    fn settings_round_trip_as_json() {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            assert_eq!(get_setting::<String>(conn, SettingKey::AiRoles)?, None);
            set_setting(
                conn,
                SettingKey::AiRoles,
                &json!({ "agent": "claude", "effort": "high" }),
            )?;
            set_setting(
                conn,
                SettingKey::AiRoles,
                &json!({ "agent": "codex", "effort": "low" }),
            )?;
            assert_eq!(
                get_setting::<serde_json::Value>(conn, SettingKey::AiRoles)?,
                Some(json!({ "agent": "codex", "effort": "low" }))
            );
            Ok(())
        })
        .unwrap();
    }
}
