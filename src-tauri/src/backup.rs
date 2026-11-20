//! Daily automatic backups and restore (spec §3.6); the export/import data file (#24) builds on
//! the same snapshots.
//!
//! A backup is a folder `backup-<UTC time>` in the backup folder, holding a consistent copy of
//! the database, the images and a manifest of sha256 hashes (see [`snapshot`]). It is written
//! to a hidden temporary folder and renamed, so a crash never leaves a half-written backup
//! that looks complete. The newest 7 are kept. Before a restore replaces anything, a
//! `safety-<UTC time>` backup of the current data is made; the newest 5 of those are kept.

pub mod datafile;
pub mod snapshot;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use rusqlite::Connection;
use serde::Serialize;
use time::OffsetDateTime;
use ts_rs::TS;

use crate::ai::settings::{KEY_CLI_PATHS, KEY_REMNOTE};
use crate::db::{Database, get_setting, set_setting};
use crate::error::{AppError, AppResult};
use crate::paths::AppPaths;

/// Setting that holds the backup folder chosen by the user.
const KEY_FOLDER: &str = "backup.folder";
/// Settings that describe this computer. Restoring or importing data from another computer
/// must not overwrite them.
pub const LOCAL_SETTINGS: &[&str] = &[KEY_FOLDER, KEY_CLI_PATHS, KEY_REMNOTE];

/// Automatic backups kept (also counts the ones made by hand).
const KEEP_BACKUPS: usize = 7;
/// Safety backups kept.
const KEEP_SAFETY: usize = 5;
/// A new automatic backup is due when the newest one is older than this.
const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_FOLDER: usize = 1_000;

/// Why a backup was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BackupKind {
    /// Daily, or "Fazer backup agora".
    Backup,
    /// Made right before a restore or import replaced the data.
    Safety,
}

impl BackupKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Backup => "backup",
            Self::Safety => "safety",
        }
    }

    fn keep(self) -> usize {
        match self {
            Self::Backup => KEEP_BACKUPS,
            Self::Safety => KEEP_SAFETY,
        }
    }
}

/// A backup in the backup folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackupInfo {
    /// Folder name; identifies the backup in `backup_restore`.
    pub name: String,
    pub kind: BackupKind,
    pub created_at: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
}

/// The *Dados* section: where backups go, the ones that exist and the last automatic failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackupStatus {
    pub folder: String,
    pub is_default_folder: bool,
    /// Newest first.
    pub backups: Vec<BackupInfo>,
    /// pt-BR reason the last automatic or manual backup failed, until one succeeds.
    pub last_error: Option<String>,
}

/// `YYYYMMDD-HHMMSS-mmm` from a timestamp like `2026-11-19T21:00:00.123Z` (see [`crate::clock`]).
fn stamp(created_at: &str) -> String {
    let digits: String = created_at.chars().filter(char::is_ascii_digit).collect();
    format!("{}-{}-{}", &digits[0..8], &digits[8..14], &digits[14..17])
}

/// The kind a folder name says it is, if it is a backup name at all.
fn parse_name(name: &str) -> Option<BackupKind> {
    let (kind, rest) = if let Some(rest) = name.strip_prefix("backup-") {
        (BackupKind::Backup, rest)
    } else {
        (BackupKind::Safety, name.strip_prefix("safety-")?)
    };
    let shape = rest.len() == 19
        && rest.bytes().enumerate().all(|(index, byte)| match index {
            8 | 15 => byte == b'-',
            _ => byte.is_ascii_digit(),
        });
    shape.then_some(kind)
}

/// The default backup folder.
pub fn default_folder(paths: &AppPaths) -> PathBuf {
    paths.data_dir().join("backups")
}

/// The folder backups go to: the user's choice, or the default.
pub fn folder(conn: &Connection, paths: &AppPaths) -> AppResult<PathBuf> {
    Ok(get_setting::<String>(conn, KEY_FOLDER)?
        .map(PathBuf::from)
        .unwrap_or_else(|| default_folder(paths)))
}

/// Saves the backup folder (`None` goes back to the default) after checking it can be written.
pub fn set_folder(conn: &Connection, paths: &AppPaths, chosen: Option<&Path>) -> AppResult<()> {
    let Some(chosen) = chosen else {
        crate::db::put_raw_setting(conn, KEY_FOLDER, None)?;
        return Ok(());
    };
    let text = chosen
        .to_str()
        .filter(|text| !text.trim().is_empty() && text.chars().count() <= MAX_FOLDER)
        .ok_or_else(|| AppError::Invalid("Escolha uma pasta válida para os backups.".into()))?;
    if !chosen.is_absolute() {
        return Err(AppError::Invalid(
            "Escolha uma pasta válida para os backups.".into(),
        ));
    }
    if chosen.starts_with(paths.images_dir()) {
        return Err(AppError::Invalid(
            "Escolha uma pasta fora da pasta de imagens do Gabaritei.".into(),
        ));
    }
    let not_writable = || {
        AppError::Invalid("Não foi possível gravar na pasta escolhida. Escolha outra pasta.".into())
    };
    fs::create_dir_all(chosen).map_err(|_| not_writable())?;
    tempfile::tempfile_in(chosen).map_err(|_| not_writable())?;
    set_setting(conn, KEY_FOLDER, &text)
}

/// Backups in `folder`, newest first. Folders that aren't complete backups are ignored.
pub fn list(folder: &Path) -> Vec<BackupInfo> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<BackupInfo> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let kind = parse_name(&name)?;
            let manifest = snapshot::read_manifest(&entry.path()).ok()?;
            Some(BackupInfo {
                name,
                kind,
                created_at: manifest.created_at.clone(),
                size_bytes: manifest.size(),
            })
        })
        .collect();
    found.sort_by(|a, b| b.name.cmp(&a.name));
    found
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs backup, restore and import one at a time, and remembers why the last backup failed.
/// Managed as Tauri state.
#[derive(Default)]
pub struct BackupService {
    busy: Mutex<()>,
    last_error: Mutex<Option<String>>,
}

impl BackupService {
    /// Runs `work` once nothing else is backing up, restoring or importing.
    pub fn exclusive<T>(&self, work: impl FnOnce() -> T) -> T {
        let _turn = lock(&self.busy);
        work()
    }

    fn remember<T>(&self, result: &AppResult<T>) {
        *lock(&self.last_error) = result.as_ref().err().map(ToString::to_string);
    }

    /// Whether a backup, restore or import is running now.
    pub fn is_busy(&self) -> bool {
        self.busy
            .try_lock()
            .is_err_and(|error| matches!(error, std::sync::TryLockError::WouldBlock))
    }

    /// The *Dados* section's data.
    pub fn status(&self, db: &Database, paths: &AppPaths) -> AppResult<BackupStatus> {
        let (folder, is_default_folder) = db.with(|conn| {
            let chosen = get_setting::<String>(conn, KEY_FOLDER)?;
            Ok((folder(conn, paths)?, chosen.is_none()))
        })?;
        Ok(BackupStatus {
            backups: list(&folder),
            folder: folder.display().to_string(),
            is_default_folder,
            last_error: lock(&self.last_error).clone(),
        })
    }

    /// Makes a backup now.
    pub fn backup_now(&self, db: &Database, paths: &AppPaths, now: &str) -> AppResult<BackupInfo> {
        self.exclusive(|| {
            let result = db
                .with(|conn| folder(conn, paths))
                .and_then(|folder| create(db, paths, &folder, BackupKind::Backup, now, true));
            self.remember(&result);
            result
        })
    }

    /// Makes a backup if the newest one is older than a day. Called at start and every hour.
    pub fn backup_if_due(
        &self,
        db: &Database,
        paths: &AppPaths,
        now: OffsetDateTime,
    ) -> AppResult<Option<BackupInfo>> {
        self.exclusive(|| {
            let result = db.with(|conn| folder(conn, paths)).and_then(|folder| {
                if is_due(&folder, now) {
                    create(
                        db,
                        paths,
                        &folder,
                        BackupKind::Backup,
                        &crate::clock::format(now),
                        true,
                    )
                    .map(Some)
                } else {
                    Ok(None)
                }
            });
            self.remember(&result);
            result
        })
    }

    /// Writes the data file to `dest`. See [`datafile::export`].
    pub fn export(&self, db: &Database, paths: &AppPaths, dest: &Path, now: &str) -> AppResult<()> {
        self.exclusive(|| datafile::export(db, paths, dest, now))
    }

    /// Replaces the local data with a data file's. See [`datafile::import`]. `ready` runs first,
    /// like in [`Self::restore`].
    pub fn import(
        &self,
        db: &Database,
        paths: &AppPaths,
        file: &Path,
        now: &str,
        ready: impl FnOnce() -> AppResult<()>,
    ) -> AppResult<()> {
        self.exclusive(|| {
            ready()?;
            let folder = db.with(|conn| folder(conn, paths))?;
            datafile::import(db, paths, &folder, file, now)
        })
    }

    /// Restores the backup (or safety backup) called `name`. See [`restore`]. `ready` runs
    /// first, once nothing else is backing up, and can refuse (for example while an AI job
    /// that writes to the database is running).
    pub fn restore(
        &self,
        db: &Database,
        paths: &AppPaths,
        name: &str,
        now: &str,
        ready: impl FnOnce() -> AppResult<()>,
    ) -> AppResult<()> {
        self.exclusive(|| {
            ready()?;
            restore(db, paths, name, now)
        })
    }
}

/// Whether the newest backup is older than a day (or there is none).
fn is_due(folder: &Path, now: OffsetDateTime) -> bool {
    let cutoff = crate::clock::format(now - INTERVAL);
    let present = crate::clock::format(now);
    list(folder)
        .iter()
        // A backup dated in the future (the clock was wrong once) must not stop new ones.
        .find(|info| info.kind == BackupKind::Backup && info.created_at <= present)
        .is_none_or(|newest| newest.created_at < cutoff)
}

/// Writes one backup into `folder`: into a hidden temporary folder first, then renamed.
fn create(
    db: &Database,
    paths: &AppPaths,
    folder: &Path,
    kind: BackupKind,
    now: &str,
    prune: bool,
) -> AppResult<BackupInfo> {
    fs::create_dir_all(folder)?;
    let name = format!("{}-{}", kind.prefix(), stamp(now));
    let finished = folder.join(&name);
    if finished.exists() {
        return Err(AppError::Invalid(
            "Já existe um backup deste instante. Tente de novo.".into(),
        ));
    }
    let building = folder.join(format!(".tmp-{name}"));
    let _ = fs::remove_dir_all(&building);
    fs::create_dir(&building)?;
    let manifest = snapshot::write(db, &paths.images_dir(), &building, now)
        .and_then(|manifest| {
            fs::rename(&building, &finished)
                .map(|()| manifest)
                .map_err(Into::into)
        })
        .inspect_err(|_| {
            let _ = fs::remove_dir_all(&building);
        })?;
    if prune {
        self::prune(folder, kind);
    }
    Ok(BackupInfo {
        name,
        kind,
        size_bytes: manifest.size(),
        created_at: manifest.created_at,
    })
}

/// Deletes the oldest backups of `kind` beyond the limit, and temporary folders left by a
/// crash. A failure to delete is not worth failing the backup that just succeeded.
fn prune(folder: &Path, kind: BackupKind) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    let mut names: Vec<String> = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with(".tmp-") {
            let _ = fs::remove_dir_all(entry.path());
        } else if parse_name(&name) == Some(kind) {
            names.push(name);
        }
    }
    names.sort();
    let excess = names.len().saturating_sub(kind.keep());
    for name in &names[..excess] {
        let _ = fs::remove_dir_all(folder.join(name));
    }
}

/// Replaces the current data with the backup called `name`:
///
/// 1. checks the backup completely (hashes, `integrity_check`, schema version): a bad backup
///    stops here and nothing changes, not even a safety backup;
/// 2. makes a safety backup of the current data;
/// 3. replaces the database and adds the images.
///
/// This computer's own settings (backup folder, CLI paths) are kept.
pub fn restore(db: &Database, paths: &AppPaths, name: &str, now: &str) -> AppResult<()> {
    if parse_name(name).is_none() {
        return Err(AppError::Invalid("Backup não encontrado.".into()));
    }
    let folder = db.with(|conn| folder(conn, paths))?;
    let source = folder.join(name);
    if !source.is_dir() {
        return Err(AppError::Invalid("Backup não encontrado.".into()));
    }
    replace_data(db, paths, &folder, &source, now)
}

/// Replaces the data with the snapshot in `source`: checks and stages it (a bad snapshot stops
/// here and nothing changes, not even a safety backup), makes a safety backup of the current
/// data, then replaces it. Shared with the import of a data file.
pub fn replace_data(
    db: &Database,
    paths: &AppPaths,
    folder: &Path,
    source: &Path,
    now: &str,
) -> AppResult<()> {
    let staged = snapshot::stage(source)?;
    // Pruned only afterwards, so a safety backup being restored is never deleted by this one.
    create(db, paths, folder, BackupKind::Safety, now, false)?;
    snapshot::commit(db, &paths.images_dir(), source, &staged, LOCAL_SETTINGS)?;
    prune(folder, BackupKind::Safety);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use time::macros::datetime;

    struct Fixture {
        _dir: tempfile::TempDir,
        paths: AppPaths,
        db: Database,
    }

    const IMAGE: &str = "0123456789abcdef01234567.png";

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().join("data"));
        let db = Database::open(&paths.database()).unwrap();
        fs::create_dir_all(paths.images_dir()).unwrap();
        fs::write(paths.images_dir().join(IMAGE), b"png bytes").unwrap();
        Fixture {
            _dir: dir,
            paths,
            db,
        }
    }

    fn add_question(db: &Database, id: &str) {
        db.with(|conn| {
            conn.execute(
                "INSERT INTO questions (id, type, stem, options, answer_id, created_at, updated_at)
                 VALUES (?1, 'boolean', 'Enunciado', '[{\"id\":\"c\",\"text\":\"Certo\"},{\"id\":\"e\",\"text\":\"Errado\"}]',
                         'c', '2026-11-19T00:00:00Z', '2026-11-19T00:00:00Z')",
                params![id],
            )?;
            Ok(())
        })
        .unwrap();
    }

    fn question_ids(db: &Database) -> Vec<String> {
        db.with(|conn| {
            let mut stmt = conn.prepare("SELECT id FROM questions ORDER BY id")?;
            Ok(stmt
                .query_map([], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()?)
        })
        .unwrap()
    }

    fn backup(f: &Fixture, now: &str) -> BackupInfo {
        let folder = default_folder(&f.paths);
        create(&f.db, &f.paths, &folder, BackupKind::Backup, now, true).unwrap()
    }

    #[test]
    fn a_consistent_backup_restores_the_answers_and_passes_the_integrity_check() {
        let f = fixture();
        add_question(&f.db, "q-1");
        add_question(&f.db, "q-2");
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        assert_eq!(info.name, "backup-20261119-100000-000");

        // The data changes after the backup: one question is lost, another one added.
        f.db.with(|conn| {
            conn.execute("DELETE FROM questions WHERE id = 'q-2'", [])?;
            Ok(())
        })
        .unwrap();
        add_question(&f.db, "q-3");

        restore(&f.db, &f.paths, &info.name, "2026-11-19T11:00:00.000Z").unwrap();

        assert_eq!(question_ids(&f.db), ["q-1", "q-2"]);
        f.db.with(|conn| {
            let integrity: String =
                conn.pragma_query_value(None, "integrity_check", |row| row.get(0))?;
            assert_eq!(integrity, "ok");
            let mode: String = conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
            assert_eq!(mode, "wal");
            Ok(())
        })
        .unwrap();
        // The database keeps working after the replacement.
        add_question(&f.db, "q-4");
    }

    #[test]
    fn restoring_makes_a_safety_backup_of_what_it_replaces() {
        let f = fixture();
        add_question(&f.db, "q-1");
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        add_question(&f.db, "q-2");

        restore(&f.db, &f.paths, &info.name, "2026-11-19T11:00:00.000Z").unwrap();

        let all = list(&default_folder(&f.paths));
        let safety = all
            .iter()
            .find(|info| info.kind == BackupKind::Safety)
            .expect("a safety backup exists");
        assert_eq!(safety.name, "safety-20261119-110000-000");
        // Restoring the safety backup brings the replaced data back.
        restore(&f.db, &f.paths, &safety.name, "2026-11-19T12:00:00.000Z").unwrap();
        assert_eq!(question_ids(&f.db), ["q-1", "q-2"]);
    }

    #[test]
    fn a_corrupt_backup_changes_nothing() {
        let f = fixture();
        add_question(&f.db, "q-1");
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        add_question(&f.db, "q-2");
        let folder = default_folder(&f.paths);
        let dir = folder.join(&info.name);

        // A flipped byte in the database fails the hash.
        let database = dir.join(snapshot::DATABASE_FILE);
        let mut bytes = fs::read(&database).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        fs::write(&database, bytes).unwrap();

        let error = restore(&f.db, &f.paths, &info.name, "2026-11-19T11:00:00.000Z").unwrap_err();
        assert!(error.to_string().contains("corrompidos"), "{error}");
        assert_eq!(question_ids(&f.db), ["q-1", "q-2"]);
        assert!(
            list(&folder).iter().all(|b| b.kind == BackupKind::Backup),
            "no safety backup for a backup that was rejected"
        );
    }

    #[test]
    fn a_backup_with_a_bad_database_or_missing_files_is_rejected() {
        let f = fixture();
        add_question(&f.db, "q-1");
        let folder = default_folder(&f.paths);

        // Missing image.
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        fs::remove_file(folder.join(&info.name).join("images").join(IMAGE)).unwrap();
        assert!(snapshot::verify(&folder.join(&info.name)).is_err());

        // A database that is not SQLite, with a matching hash.
        let info = backup(&f, "2026-11-19T10:01:00.000Z");
        let dir = folder.join(&info.name);
        let database = dir.join(snapshot::DATABASE_FILE);
        fs::write(&database, b"definitely not a database").unwrap();
        let mut manifest = snapshot::read_manifest(&dir).unwrap();
        let (sha256, size) = snapshot::hash_file(&database).unwrap();
        manifest.files[0].sha256 = sha256;
        manifest.files[0].size = size;
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(snapshot::verify(&dir).is_err());

        // A manifest that points outside the snapshot.
        let info = backup(&f, "2026-11-19T10:02:00.000Z");
        let dir = folder.join(&info.name);
        let mut manifest = snapshot::read_manifest(&dir).unwrap();
        manifest.files[0].path = "../gabaritei.sqlite".into();
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(snapshot::verify(&dir).is_err());

        assert_eq!(question_ids(&f.db), ["q-1"]);
    }

    /// Runs `sql` on a backup's database and fixes the manifest hash, as someone building a
    /// hostile file would.
    fn doctor(f: &Fixture, info: &BackupInfo, sql: &str) {
        let dir = default_folder(&f.paths).join(&info.name);
        let database = dir.join(snapshot::DATABASE_FILE);
        Connection::open(&database)
            .unwrap()
            .execute_batch(sql)
            .unwrap();
        let mut manifest = snapshot::read_manifest(&dir).unwrap();
        let (sha256, size) = snapshot::hash_file(&database).unwrap();
        manifest.files[0].sha256 = sha256;
        manifest.files[0].size = size;
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn a_database_with_another_schema_is_rejected_before_anything_changes() {
        for sql in [
            "DROP TABLE settings",
            "CREATE TRIGGER evil AFTER INSERT ON responses BEGIN DELETE FROM settings; END",
            "CREATE VIEW sneaky AS SELECT 1",
            // A name that a `LIKE 'sqlite_%'` filter would have taken for an internal one.
            "CREATE TRIGGER sqliteXevil AFTER INSERT ON responses BEGIN DELETE FROM settings; END",
            "CREATE TABLE sqliteXextra (x TEXT) STRICT",
            "CREATE TABLE extra (x TEXT) STRICT",
            // A row whose `type` says index but whose SQL is a trigger, as a hand-built file could.
            "PRAGMA writable_schema = ON; INSERT INTO sqlite_schema VALUES ('index', 'sqlite_evil', 'responses', 0, 'CREATE TRIGGER sqlite_evil AFTER INSERT ON responses BEGIN DELETE FROM settings; END')",
        ] {
            let f = fixture();
            add_question(&f.db, "q-1");
            let info = backup(&f, "2026-11-19T10:00:00.000Z");
            doctor(&f, &info, sql);
            add_question(&f.db, "q-2");
            let folder = default_folder(&f.paths);

            let error =
                restore(&f.db, &f.paths, &info.name, "2026-11-19T11:00:00.000Z").unwrap_err();

            assert!(
                error.to_string().contains("Nada foi alterado"),
                "{sql}: {error}"
            );
            assert_eq!(question_ids(&f.db), ["q-1", "q-2"], "{sql}");
            assert!(
                list(&folder).iter().all(|b| b.kind == BackupKind::Backup),
                "{sql}: no safety backup"
            );
            // The app still works: its settings and its backup folder are intact.
            f.db.with(|conn| folder_of(conn, &f.paths)).unwrap();
        }
    }

    fn folder_of(conn: &Connection, paths: &AppPaths) -> AppResult<PathBuf> {
        folder(conn, paths)
    }

    #[test]
    fn a_backup_from_a_newer_schema_is_rejected() {
        let f = fixture();
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        let dir = default_folder(&f.paths).join(&info.name);
        let mut manifest = snapshot::read_manifest(&dir).unwrap();
        manifest.schema_version = crate::db::SCHEMA_VERSION + 1;
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let error = snapshot::verify(&dir).unwrap_err();
        assert_eq!(error.to_string(), crate::db::NEWER_SCHEMA_MESSAGE);
    }

    #[test]
    fn restore_only_accepts_backup_names() {
        let f = fixture();
        for name in [
            "../x",
            "backup-1",
            "other",
            "backup-20261119-100000-00x",
            "",
        ] {
            assert!(restore(&f.db, &f.paths, name, "2026-11-19T11:00:00.000Z").is_err());
        }
    }

    #[test]
    fn images_are_backed_up_and_come_back() {
        let f = fixture();
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        let dir = default_folder(&f.paths).join(&info.name);
        assert_eq!(
            fs::read(dir.join("images").join(IMAGE)).unwrap(),
            b"png bytes"
        );
        // Not an image name: never copied.
        fs::write(f.paths.images_dir().join("notes.txt"), b"x").unwrap();
        let second = backup(&f, "2026-11-19T10:01:00.000Z");
        assert!(
            !default_folder(&f.paths)
                .join(&second.name)
                .join("images/notes.txt")
                .exists()
        );

        fs::remove_file(f.paths.images_dir().join(IMAGE)).unwrap();
        restore(&f.db, &f.paths, &info.name, "2026-11-19T11:00:00.000Z").unwrap();
        assert_eq!(
            fs::read(f.paths.images_dir().join(IMAGE)).unwrap(),
            b"png bytes"
        );
    }

    #[test]
    fn restoring_keeps_this_computers_own_settings() {
        let f = fixture();
        f.db.with(|conn| {
            set_setting(conn, KEY_CLI_PATHS, &"windows-path")?;
            set_setting(conn, KEY_REMNOTE, &"http://remote.example/mcp")?;
            set_setting(conn, "other", &"from the backup")
        })
        .unwrap();
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        f.db.with(|conn| {
            set_setting(conn, KEY_CLI_PATHS, &"mac-path")?;
            set_setting(conn, KEY_REMNOTE, &"http://localhost/mcp")?;
            set_setting(conn, "other", &"changed")?;
            set_setting(conn, KEY_FOLDER, &"/elsewhere")
        })
        .unwrap();

        // The folder setting points at /elsewhere now, so the backup is found by moving it.
        let elsewhere = tempfile::tempdir().unwrap();
        f.db.with(|conn| set_setting(conn, KEY_FOLDER, &elsewhere.path().to_str().unwrap()))
            .unwrap();
        fs::rename(
            default_folder(&f.paths).join(&info.name),
            elsewhere.path().join(&info.name),
        )
        .unwrap();
        restore(&f.db, &f.paths, &info.name, "2026-11-19T11:00:00.000Z").unwrap();

        f.db.with(|conn| {
            assert_eq!(
                get_setting::<String>(conn, "other")?.as_deref(),
                Some("from the backup")
            );
            assert_eq!(
                get_setting::<String>(conn, KEY_CLI_PATHS)?.as_deref(),
                Some("mac-path")
            );
            assert_eq!(
                get_setting::<String>(conn, KEY_REMNOTE)?.as_deref(),
                Some("http://localhost/mcp")
            );
            assert_eq!(
                get_setting::<String>(conn, KEY_FOLDER)?.as_deref(),
                elsewhere.path().to_str()
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn restoring_rebuilds_reviews_from_the_restored_answers() {
        let f = fixture();
        add_question(&f.db, "q-1");
        // A review the answers don't justify, as an older build could have left in a backup.
        f.db.with(|conn| {
            conn.execute(
                "INSERT INTO review_state (question_id, due_at, state)
                 VALUES ('q-1', '2026-11-20T00:00:00Z', '{}')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        restore(&f.db, &f.paths, &info.name, "2026-11-19T11:00:00.000Z").unwrap();
        let reviews: i64 = f
            .db
            .with(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM review_state", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(reviews, 0);
    }

    #[test]
    fn keeps_the_newest_seven_backups_and_five_safety_backups() {
        let f = fixture();
        for day in 1..=9 {
            backup(&f, &format!("2026-11-{day:02}T10:00:00.000Z"));
        }
        let folder = default_folder(&f.paths);
        let names: Vec<String> = list(&folder).into_iter().map(|b| b.name).collect();
        assert_eq!(names.len(), 7);
        assert_eq!(names[0], "backup-20261109-100000-000");
        assert_eq!(names[6], "backup-20261103-100000-000");

        for minute in 0..7 {
            restore(
                &f.db,
                &f.paths,
                "backup-20261109-100000-000",
                &format!("2026-11-10T10:{minute:02}:00.000Z"),
            )
            .unwrap();
        }
        let all = list(&folder);
        assert_eq!(
            all.iter().filter(|b| b.kind == BackupKind::Safety).count(),
            5
        );
        assert_eq!(
            all.iter().filter(|b| b.kind == BackupKind::Backup).count(),
            7
        );
    }

    #[test]
    fn a_backup_is_due_after_a_day_and_ignores_safety_backups_and_leftovers() {
        let f = fixture();
        let folder = default_folder(&f.paths);
        let service = BackupService::default();
        let at = |text: OffsetDateTime| service.backup_if_due(&f.db, &f.paths, text).unwrap();

        // None yet: one is made. Right after, nothing is due.
        assert!(at(datetime!(2026-11-19 10:00:00 UTC)).is_some());
        assert!(at(datetime!(2026-11-19 11:00:00 UTC)).is_none());
        assert!(at(datetime!(2026-11-20 09:59:59 UTC)).is_none());
        assert!(at(datetime!(2026-11-20 10:00:01 UTC)).is_some());
        assert_eq!(list(&folder).len(), 2);

        // A leftover temporary folder is not a backup, and is cleaned at the next backup.
        fs::create_dir_all(folder.join(".tmp-backup-20261201-000000-000")).unwrap();
        assert_eq!(list(&folder).len(), 2);
        assert!(at(datetime!(2026-11-22 10:00:00 UTC)).is_some());
        assert!(!folder.join(".tmp-backup-20261201-000000-000").exists());
    }

    #[test]
    fn a_backup_dated_in_the_future_does_not_stop_new_ones() {
        let f = fixture();
        let service = BackupService::default();
        // The clock was wrong once and made a backup dated far ahead.
        backup(&f, "2030-01-01T10:00:00.000Z");
        let made = service
            .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 10:00:00 UTC))
            .unwrap();
        assert!(made.is_some());
    }

    #[test]
    fn a_failed_backup_leaves_no_partial_folder_and_is_reported() {
        let f = fixture();
        let service = BackupService::default();
        // The backup folder is a file: nothing can be created in it.
        let blocker = f.paths.data_dir().join("blocked");
        fs::write(&blocker, b"file").unwrap();
        f.db.with(|conn| set_setting(conn, KEY_FOLDER, &blocker.to_str().unwrap()))
            .unwrap();

        assert!(
            service
                .backup_now(&f.db, &f.paths, "2026-11-19T10:00:00.000Z")
                .is_err()
        );
        let status = service.status(&f.db, &f.paths).unwrap();
        assert!(status.last_error.is_some());

        f.db.with(|conn| {
            set_setting(
                conn,
                KEY_FOLDER,
                &f.paths.data_dir().join("ok").to_str().unwrap(),
            )
        })
        .unwrap();
        service
            .backup_now(&f.db, &f.paths, "2026-11-19T10:00:00.000Z")
            .unwrap();
        assert_eq!(service.status(&f.db, &f.paths).unwrap().last_error, None);
    }

    #[test]
    fn the_folder_must_be_absolute_and_writable() {
        let f = fixture();
        f.db.with(|conn| {
            assert!(set_folder(conn, &f.paths, Some(Path::new("relative/dir"))).is_err());
            assert!(set_folder(conn, &f.paths, Some(Path::new(""))).is_err());
            assert!(set_folder(conn, &f.paths, Some(&f.paths.images_dir())).is_err());

            let chosen = f.paths.data_dir().join("my backups");
            set_folder(conn, &f.paths, Some(&chosen))?;
            assert_eq!(folder(conn, &f.paths)?, chosen);
            assert!(chosen.is_dir());

            set_folder(conn, &f.paths, None)?;
            assert_eq!(folder(conn, &f.paths)?, default_folder(&f.paths));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn backup_names_round_trip() {
        assert_eq!(stamp("2026-11-19T21:00:00.123Z"), "20261119-210000-123");
        assert_eq!(
            parse_name("backup-20261119-210000-123"),
            Some(BackupKind::Backup)
        );
        assert_eq!(
            parse_name("safety-20261119-210000-123"),
            Some(BackupKind::Safety)
        );
        assert_eq!(parse_name("backup-20261119-210000-12"), None);
        assert_eq!(parse_name(".tmp-backup-20261119-210000-123"), None);
    }
}
