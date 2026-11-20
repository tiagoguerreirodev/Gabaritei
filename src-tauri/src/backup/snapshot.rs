//! A snapshot is a folder with a consistent copy of the database, the images and a manifest of
//! sha256 hashes. Backups are snapshots; the export file (#24) is a snapshot in a zip.
//!
//! Nothing in a snapshot is trusted until [`verify`] has passed: every file the manifest lists
//! must exist with the recorded hash, the database must pass `integrity_check` and its schema
//! must not be newer than this build's.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::bank::question::is_valid_image_name;
use crate::db::{self, Database};
use crate::error::{AppError, AppResult};

/// File name of the manifest inside a snapshot.
pub const MANIFEST_FILE: &str = "manifest.json";
/// Database file inside a snapshot.
pub const DATABASE_FILE: &str = "gabaritei.sqlite";
/// Images folder inside a snapshot.
pub const IMAGES_DIR: &str = "images";
/// Version of the manifest layout.
pub const FORMAT: u32 = 1;

/// Shown whenever a snapshot can't be trusted. Nothing was changed.
pub const CORRUPT_MESSAGE: &str =
    "A cópia de segurança está corrompida ou incompleta. Nada foi alterado.";

/// One file of a snapshot, with the hash it must have.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFile {
    /// Path relative to the snapshot: the database or `images/<name>`.
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

/// What a snapshot holds and when it was made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: u32,
    pub app_version: String,
    pub created_at: String,
    pub schema_version: u32,
    pub files: Vec<ManifestFile>,
}

impl Manifest {
    /// Total size of the files.
    pub fn size(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }
}

fn corrupt() -> AppError {
    AppError::Invalid(CORRUPT_MESSAGE.into())
}

/// The sha256 of a file, as hex, and its size. Streams, so images never sit in memory twice.
pub fn hash_file(path: &Path) -> AppResult<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    Ok((hex::encode(hasher.finalize()), size))
}

/// Copies the live database into `dest` with SQLite's online backup, so the copy is consistent
/// even while the app writes. The copy is a single file (no WAL).
fn copy_database(conn: &Connection, dest: &Path) -> AppResult<()> {
    let mut target = Connection::open(dest)?;
    {
        let backup = Backup::new(conn, &mut target)?;
        backup.run_to_completion(256, Duration::ZERO, None)?;
    }
    let _mode: String =
        target.pragma_update_and_check(None, "journal_mode", "delete", |row| row.get(0))?;
    target.close().map_err(|(_, error)| AppError::from(error))?;
    Ok(())
}

/// Writes a snapshot of the database and images into `dest`, which must be an empty folder.
pub fn write(
    database: &Database,
    images_dir: &Path,
    dest: &Path,
    created_at: &str,
) -> AppResult<Manifest> {
    fs::create_dir_all(dest.join(IMAGES_DIR))?;
    // The database first: every image it references already exists, and newer ones are harmless.
    let schema_version = database.with(|conn| {
        copy_database(conn, &dest.join(DATABASE_FILE))?;
        db::user_version(conn)
    })?;
    let mut files = Vec::new();
    let (sha256, size) = hash_file(&dest.join(DATABASE_FILE))?;
    files.push(ManifestFile {
        path: DATABASE_FILE.into(),
        sha256,
        size,
    });
    let mut images = match fs::read_dir(images_dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| is_valid_image_name(name))
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.into()),
    };
    images.sort();
    for name in images {
        let target = dest.join(IMAGES_DIR).join(&name);
        fs::copy(images_dir.join(&name), &target)?;
        let (sha256, size) = hash_file(&target)?;
        files.push(ManifestFile {
            path: format!("{IMAGES_DIR}/{name}"),
            sha256,
            size,
        });
    }
    let manifest = Manifest {
        format: FORMAT,
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        created_at: created_at.to_owned(),
        schema_version,
        files,
    };
    fs::write(
        dest.join(MANIFEST_FILE),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(manifest)
}

/// Reads a snapshot's manifest without checking the files (enough to list backups).
pub fn read_manifest(dir: &Path) -> AppResult<Manifest> {
    let bytes = fs::read(dir.join(MANIFEST_FILE)).map_err(|_| corrupt())?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| corrupt())?;
    if manifest.format != FORMAT {
        return Err(corrupt());
    }
    Ok(manifest)
}

/// Whether a manifest path is the database or `images/<valid name>`: nothing else is ever read.
fn is_allowed_path(path: &str) -> bool {
    path == DATABASE_FILE
        || path
            .strip_prefix("images/")
            .is_some_and(is_valid_image_name)
}

/// Checks a snapshot completely and returns its manifest. Changes nothing.
pub fn verify(dir: &Path) -> AppResult<Manifest> {
    let manifest = read_manifest(dir)?;
    if manifest.schema_version > db::SCHEMA_VERSION {
        return Err(AppError::Invalid(db::NEWER_SCHEMA_MESSAGE.into()));
    }
    let mut seen = HashSet::new();
    for file in &manifest.files {
        if !is_allowed_path(&file.path) || !seen.insert(file.path.as_str()) {
            return Err(corrupt());
        }
        let path = snapshot_path(dir, &file.path);
        let (sha256, size) = hash_file(&path).map_err(|_| corrupt())?;
        if sha256 != file.sha256 || size != file.size {
            return Err(corrupt());
        }
    }
    if !seen.contains(DATABASE_FILE) {
        return Err(corrupt());
    }
    check_database(&dir.join(DATABASE_FILE), manifest.schema_version)?;
    Ok(manifest)
}

fn snapshot_path(dir: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(dir.to_path_buf(), |path, part| path.join(part))
}

/// The database file must open, pass `integrity_check` and `foreign_key_check`, and have the
/// schema version the manifest claims (never newer than this build's).
fn check_database(path: &Path, expected_version: u32) -> AppResult<()> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| corrupt())?;
    let integrity: String = conn
        .pragma_query_value(None, "integrity_check", |row| row.get(0))
        .map_err(|_| corrupt())?;
    if integrity != "ok" {
        return Err(corrupt());
    }
    let version = db::user_version(&conn).map_err(|_| corrupt())?;
    if version > db::SCHEMA_VERSION {
        return Err(AppError::Invalid(db::NEWER_SCHEMA_MESSAGE.into()));
    }
    if version != expected_version {
        return Err(corrupt());
    }
    let broken: Option<String> = conn
        .query_row("PRAGMA foreign_key_check", [], |row| row.get(0))
        .map(Some)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(|_| corrupt())?;
    if broken.is_some() {
        return Err(corrupt());
    }
    Ok(())
}

/// A snapshot's database, copied out of its folder and checked, ready to replace the live one.
pub struct Staged {
    dir: tempfile::TempDir,
    manifest: Manifest,
}

impl Staged {
    fn database(&self) -> PathBuf {
        self.dir.path().join(DATABASE_FILE)
    }
}

/// The tables, indexes, triggers and views of this build's schema, to compare a file's with.
fn expected_schema() -> AppResult<Vec<(String, String, Option<String>)>> {
    Database::open_in_memory()?.with(|conn| read_schema(conn))
}

fn read_schema(conn: &Connection) -> AppResult<Vec<(String, String, Option<String>)>> {
    let mut stmt = conn.prepare("SELECT type, name, sql FROM sqlite_schema ORDER BY type, name")?;
    Ok(stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<Vec<_>, _>>()?)
}

/// Checks a snapshot and copies its database to a private temporary folder, where it is
/// migrated to this build's schema and compared with it. Nothing local changes here, so any
/// failure means "nothing was changed".
///
/// The copy matters: a backup folder may be synced by a cloud client that rewrites files after
/// they were verified, and a file built by someone else may have a schema this app never
/// created (a missing table, a trigger). Only a database with exactly this app's schema gets
/// past this point.
pub fn stage(dir: &Path) -> AppResult<Staged> {
    let manifest = verify(dir)?;
    let staged = Staged {
        dir: tempfile::tempdir()?,
        manifest,
    };
    fs::copy(dir.join(DATABASE_FILE), staged.database())?;
    let expected = staged
        .manifest
        .files
        .iter()
        .find(|file| file.path == DATABASE_FILE)
        .ok_or_else(corrupt)?;
    let (sha256, _) = hash_file(&staged.database())?;
    if sha256 != expected.sha256 {
        return Err(corrupt());
    }
    let mut conn = Connection::open(staged.database()).map_err(|_| corrupt())?;
    // This app's schema has only tables and indexes, so anything else can only come from the
    // file. The `type` column is just data in a hand-built file, while SQLite loads an object
    // from its `sql` text, so both are checked. Refused before anything runs on the file,
    // migrations included.
    let foreign: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE
                 type NOT IN ('table', 'index')
              OR (sql IS NOT NULL
                  AND NOT (upper(ltrim(sql)) LIKE 'CREATE TABLE %'
                           OR upper(ltrim(sql)) LIKE 'CREATE INDEX %'
                           OR upper(ltrim(sql)) LIKE 'CREATE UNIQUE INDEX %'))
              OR (substr(name, 1, 7) = 'sqlite_'
                  AND NOT (name = 'sqlite_sequence'
                           OR substr(name, 1, 11) = 'sqlite_stat'
                           OR (substr(name, 1, 16) = 'sqlite_autoindex' AND sql IS NULL)))",
            [],
            |row| row.get(0),
        )
        .map_err(|_| corrupt())?;
    if foreign != 0 {
        return Err(corrupt());
    }
    db::prepare_restored(&mut conn).map_err(|error| match error {
        AppError::Invalid(_) => error,
        _ => corrupt(),
    })?;
    if read_schema(&conn).map_err(|_| corrupt())? != expected_schema()? {
        return Err(corrupt());
    }
    Ok(staged)
}

/// Replaces the live database with a staged one, and adds the snapshot's images.
///
/// Images go first, each checked against its hash, so the restored database never points at a
/// missing file; existing images stay (names are content hashes). Then, on the staged copy
/// only, this computer's own settings are put back, what derives from the answers is rebuilt,
/// and a blind check that was running in the replaced data is marked as interrupted. Last,
/// SQLite copies the staged pages into the live connection inside one write transaction, so a
/// failure leaves the old data and a reader never sees a half-restored database.
///
/// `keep_local` lists settings that describe this computer (folders, CLI paths): their current
/// values survive the replacement, and a key this computer never set is removed again.
pub fn commit(
    database: &Database,
    images_dir: &Path,
    dir: &Path,
    staged: &Staged,
    keep_local: &[&str],
) -> AppResult<()> {
    fs::create_dir_all(images_dir)?;
    for file in &staged.manifest.files {
        let Some(name) = file.path.strip_prefix("images/") else {
            continue;
        };
        let target = images_dir.join(name);
        if target.exists() {
            continue;
        }
        let partial = images_dir.join(format!("{name}.partial"));
        fs::copy(snapshot_path(dir, &file.path), &partial)?;
        let (sha256, _) = hash_file(&partial)?;
        if sha256 != file.sha256 {
            let _ = fs::remove_file(&partial);
            return Err(corrupt());
        }
        fs::rename(&partial, &target)?;
    }
    database.with(|conn| {
        let mut target = Connection::open(staged.database())?;
        for key in keep_local {
            let local = db::get_raw_setting(conn, key)?;
            db::put_raw_setting(&target, key, local.as_deref())?;
        }
        {
            let tx = target.transaction()?;
            crate::ai::blind_check::recover_interrupted(&tx)?;
            crate::learning::refresh_all(&tx)?;
            tx.commit()?;
        }
        let integrity: String =
            target.pragma_query_value(None, "integrity_check", |row| row.get(0))?;
        if integrity != "ok" {
            return Err(corrupt());
        }
        Backup::new(&target, conn)?.run_to_completion(256, Duration::ZERO, None)?;
        // The pages came from a file with its own journal mode.
        db::prepare_restored(conn)
    })
}
