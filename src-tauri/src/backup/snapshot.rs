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

/// Replaces the live database with the one in a verified snapshot, and adds its images.
///
/// SQLite copies the pages inside one write transaction of the live connection, so a failure
/// leaves the old data untouched and a reader never sees a half-restored database. Images are
/// copied first, so the restored database never points at a missing file. Existing images stay:
/// their names are content hashes, and one the restored data doesn't use is harmless.
///
/// `keep_local` lists settings that describe this computer (folders, CLI paths): their current
/// values survive the replacement, and a key this computer never set is removed again.
pub fn apply(
    database: &Database,
    images_dir: &Path,
    dir: &Path,
    keep_local: &[&str],
) -> AppResult<()> {
    let manifest = verify(dir)?;
    fs::create_dir_all(images_dir)?;
    for file in &manifest.files {
        let Some(name) = file.path.strip_prefix("images/") else {
            continue;
        };
        let target = images_dir.join(name);
        if target.exists() {
            continue;
        }
        let partial = images_dir.join(format!("{name}.partial"));
        fs::copy(snapshot_path(dir, &file.path), &partial)?;
        fs::rename(&partial, &target)?;
    }
    database.with(|conn| {
        let local = keep_local
            .iter()
            .map(|key| Ok((*key, db::get_raw_setting(conn, key)?)))
            .collect::<AppResult<Vec<_>>>()?;
        {
            let source = Connection::open_with_flags(
                dir.join(DATABASE_FILE),
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            Backup::new(&source, conn)?.run_to_completion(256, Duration::ZERO, None)?;
        }
        db::prepare_restored(conn)?;
        for (key, value) in local {
            db::put_raw_setting(conn, key, value.as_deref())?;
        }
        Ok(())
    })
}
