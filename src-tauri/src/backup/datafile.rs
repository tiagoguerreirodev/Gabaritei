//! The data file `Gabaritei-<date>.gabaritei` (spec §3.6): a zip of one snapshot, to move the
//! study data between the two computers. Importing replaces local data; there is no merging.
//!
//! The file is untrusted. It is unpacked into a fresh temporary folder, only the names a
//! snapshot has are accepted (so no path can escape), sizes are capped, and the snapshot is
//! verified completely before anything local is touched.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::snapshot::{self, DATABASE_FILE, IMAGES_DIR, MANIFEST_FILE};
use crate::bank::question::is_valid_image_name;
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::paths::AppPaths;

/// File extension of the data file.
pub const EXTENSION: &str = "gabaritei";

/// How much a file may unpack to, so a crafted file can't fill the disk: each database or
/// image, and everything together.
#[derive(Debug, Clone, Copy)]
struct Limits {
    database: u64,
    image: u64,
    total: u64,
}

const LIMITS: Limits = Limits {
    database: 4 * 1024 * 1024 * 1024,
    image: 100 * 1024 * 1024,
    total: 8 * 1024 * 1024 * 1024,
};
/// A snapshot has the manifest, the database and the images.
const MAX_ENTRIES: usize = 200_000;

const NOT_A_DATA_FILE: &str = "Este não é um arquivo de dados do Gabaritei. Nada foi alterado.";

/// The suggested name: `Gabaritei-2026-11-19.gabaritei`, from a timestamp like `2026-11-19T…`.
pub fn file_name(now: &str) -> String {
    let date = now.get(..10).unwrap_or("dados");
    format!("Gabaritei-{date}.{EXTENSION}")
}

fn not_a_data_file() -> AppError {
    AppError::Invalid(NOT_A_DATA_FILE.into())
}

/// Writes a data file at `dest`: into a partial file first, then renamed over `dest`.
pub fn export(database: &Database, paths: &AppPaths, dest: &Path, now: &str) -> AppResult<()> {
    // Some save dialogs don't add the extension, and the import dialog only lists this one.
    let with_extension;
    let dest = if dest
        .extension()
        .is_some_and(|extension| extension == EXTENSION)
    {
        dest
    } else {
        with_extension = dest.with_added_extension(EXTENSION);
        &with_extension
    };
    let staging = tempfile::tempdir()?;
    let manifest = snapshot::write(database, &paths.images_dir(), staging.path(), now)?;

    let mut partial = dest.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    let written = write_zip(staging.path(), &manifest, &partial);
    if written.is_err() {
        let _ = fs::remove_file(&partial);
    }
    written?;
    fs::rename(&partial, dest).inspect_err(|_| {
        let _ = fs::remove_file(&partial);
    })?;
    Ok(())
}

fn write_zip(dir: &Path, manifest: &snapshot::Manifest, target: &Path) -> AppResult<()> {
    let zip_error = |error: zip::result::ZipError| AppError::Internal(error.to_string());
    let mut writer = ZipWriter::new(File::create(target)?);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut add = |name: &str, path: &Path| -> AppResult<()> {
        writer.start_file(name, options).map_err(zip_error)?;
        io::copy(&mut File::open(path)?, &mut writer)?;
        Ok(())
    };
    add(MANIFEST_FILE, &dir.join(MANIFEST_FILE))?;
    for file in &manifest.files {
        let path = file
            .path
            .split('/')
            .fold(dir.to_path_buf(), |path, part| path.join(part));
        add(&file.path, &path)?;
    }
    writer.finish().map_err(zip_error)?.sync_all()?;
    Ok(())
}

/// Copies one zip entry to `out`. A failure to *read* the entry (a bad checksum, a truncated or
/// damaged archive) means the file is not a good data file; only a failure to *write* is an
/// unexpected error (a full disk, say).
fn copy_entry(entry: &mut impl Read, out: &mut impl Write) -> AppResult<u64> {
    let mut buffer = [0u8; 64 * 1024];
    let mut copied = 0u64;
    loop {
        let read = match entry.read(&mut buffer) {
            Ok(0) => return Ok(copied),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(not_a_data_file()),
        };
        out.write_all(&buffer[..read])?;
        copied += read as u64;
    }
}

/// Unpacks a data file into the empty folder `into`, accepting only what a snapshot contains.
pub fn unpack(file: &Path, into: &Path) -> AppResult<()> {
    unpack_within(file, into, LIMITS)
}

fn unpack_within(file: &Path, into: &Path, limits: Limits) -> AppResult<()> {
    let mut remaining = limits.total;
    let mut archive = ZipArchive::new(File::open(file)?).map_err(|_| not_a_data_file())?;
    if archive.len() > MAX_ENTRIES {
        return Err(not_a_data_file());
    }
    fs::create_dir_all(into.join(IMAGES_DIR))?;
    let mut seen = std::collections::HashSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|_| not_a_data_file())?;
        let name = entry.name().map_err(|_| not_a_data_file())?.into_owned();
        if entry.is_dir() {
            continue;
        }
        let limit = if name == MANIFEST_FILE {
            // The manifest lists every file, so it grows with the images.
            64 * 1024 * 1024
        } else if name == DATABASE_FILE {
            limits.database
        } else if name
            .strip_prefix("images/")
            .is_some_and(is_valid_image_name)
        {
            limits.image
        } else {
            return Err(not_a_data_file());
        };
        if !seen.insert(name.clone()) {
            return Err(not_a_data_file());
        }
        let target = name
            .split('/')
            .fold(into.to_path_buf(), |path, part| path.join(part));
        let mut out = File::create(&target)?;
        // One byte more than allowed tells "exactly at the limit" from "over it".
        let limit = limit.min(remaining);
        let copied = copy_entry(&mut (&mut entry).take(limit + 1), &mut out)?;
        out.flush()?;
        if copied > limit {
            return Err(not_a_data_file());
        }
        remaining -= copied;
    }
    if !seen.contains(MANIFEST_FILE) {
        return Err(not_a_data_file());
    }
    Ok(())
}

/// Replaces the local data with a data file's: unpack, then the same check, safety backup and
/// replacement as a restore. Any
/// problem before the replacement leaves everything as it was.
pub fn import(
    database: &Database,
    paths: &AppPaths,
    backup_folder: &Path,
    file: &Path,
    now: &str,
) -> AppResult<()> {
    let staging = tempfile::tempdir()?;
    unpack(file, staging.path())?;
    super::replace_data(database, paths, backup_folder, staging.path(), now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::{BackupKind, default_folder, list};
    use crate::db::{get_setting, set_setting};
    use rusqlite::params;

    const IMAGE: &str = "0123456789abcdef01234567.png";
    const NOW: &str = "2026-11-19T10:00:00.000Z";

    /// A profile: its own app data folder and database, like one computer.
    struct Profile {
        _dir: tempfile::TempDir,
        paths: AppPaths,
        db: Database,
    }

    fn profile() -> Profile {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().join("data"));
        let db = Database::open(&paths.database()).unwrap();
        fs::create_dir_all(paths.images_dir()).unwrap();
        Profile {
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

    /// Question ids with their answer keys.
    fn questions(db: &Database) -> Vec<(String, String)> {
        db.with(|conn| {
            let mut stmt = conn.prepare("SELECT id, answer_id FROM questions ORDER BY id")?;
            Ok(stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap()
    }

    fn import_into(target: &Profile, file: &Path) -> AppResult<()> {
        import(
            &target.db,
            &target.paths,
            &default_folder(&target.paths),
            file,
            NOW,
        )
    }

    #[test]
    fn the_file_name_has_the_date() {
        assert_eq!(
            file_name("2026-11-19T21:00:00.123Z"),
            "Gabaritei-2026-11-19.gabaritei"
        );
    }

    #[test]
    fn a_round_trip_gives_the_same_counts_and_answers_in_a_fresh_profile() {
        let source = profile();
        add_question(&source.db, "q-1");
        add_question(&source.db, "q-2");
        fs::write(source.paths.images_dir().join(IMAGE), b"png bytes").unwrap();
        let out = tempfile::tempdir().unwrap();
        let file = out.path().join(file_name(NOW));

        export(&source.db, &source.paths, &file, NOW).unwrap();
        assert!(file.is_file());
        assert!(
            !out.path()
                .join("Gabaritei-2026-11-19.gabaritei.partial")
                .exists()
        );

        let fresh = profile();
        import_into(&fresh, &file).unwrap();

        assert_eq!(questions(&fresh.db), questions(&source.db));
        assert_eq!(
            fs::read(fresh.paths.images_dir().join(IMAGE)).unwrap(),
            b"png bytes"
        );
        fresh
            .db
            .with(|conn| {
                let integrity: String =
                    conn.pragma_query_value(None, "integrity_check", |row| row.get(0))?;
                assert_eq!(integrity, "ok");
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn importing_replaces_local_data_without_merging_and_keeps_a_safety_backup() {
        let source = profile();
        add_question(&source.db, "from-source");
        let out = tempfile::tempdir().unwrap();
        let file = out.path().join("data.gabaritei");
        export(&source.db, &source.paths, &file, NOW).unwrap();

        let target = profile();
        add_question(&target.db, "local-only");
        source
            .db
            .with(|conn| set_setting(conn, "backup.folder", &"/the/other/computer"))
            .unwrap();
        target
            .db
            .with(|conn| set_setting(conn, "ai.cliPaths", &"local cli paths"))
            .unwrap();
        import_into(&target, &file).unwrap();

        let ids: Vec<String> = questions(&target.db)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(ids, ["from-source"]);
        let safety: Vec<_> = list(&default_folder(&target.paths))
            .into_iter()
            .filter(|b| b.kind == BackupKind::Safety)
            .collect();
        assert_eq!(safety.len(), 1);
        // The other computer's paths do not come along.
        target
            .db
            .with(|conn| {
                assert_eq!(
                    get_setting::<String>(conn, "ai.cliPaths")?.as_deref(),
                    Some("local cli paths")
                );
                assert_eq!(get_setting::<String>(conn, "backup.folder")?, None);
                Ok(())
            })
            .unwrap();
    }

    /// Rewrites a data file with `change` applied to its entries: (name, bytes) pairs.
    fn tamper(
        file: &Path,
        into: &Path,
        change: impl FnOnce(&mut Vec<(String, Vec<u8>)>),
    ) -> PathBuf {
        let mut archive = ZipArchive::new(File::open(file).unwrap()).unwrap();
        let mut entries = Vec::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            entries.push((entry.name().unwrap().into_owned(), bytes));
        }
        change(&mut entries);
        let path = into.join("tampered.gabaritei");
        let mut writer = ZipWriter::new(File::create(&path).unwrap());
        for (name, bytes) in entries {
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(&bytes).unwrap();
        }
        writer.finish().unwrap();
        path
    }

    #[test]
    fn an_invalid_file_is_rejected_and_nothing_changes() {
        let source = profile();
        add_question(&source.db, "from-source");
        fs::write(source.paths.images_dir().join(IMAGE), b"png bytes").unwrap();
        let out = tempfile::tempdir().unwrap();
        let good = out.path().join("good.gabaritei");
        export(&source.db, &source.paths, &good, NOW).unwrap();

        let target = profile();
        add_question(&target.db, "local-only");
        let before = questions(&target.db);

        // Not a zip at all.
        let garbage = out.path().join("garbage.gabaritei");
        fs::write(&garbage, b"this is not a zip file").unwrap();
        let error = import_into(&target, &garbage).unwrap_err();
        assert_eq!(error.to_string(), NOT_A_DATA_FILE);

        // A flipped byte in the database fails the hash.
        let corrupt = tamper(&good, out.path(), |entries| {
            let database = entries
                .iter_mut()
                .find(|(n, _)| n == DATABASE_FILE)
                .unwrap();
            let last = database.1.len() - 1;
            database.1[last] ^= 0xFF;
        });
        assert!(
            import_into(&target, &corrupt)
                .unwrap_err()
                .to_string()
                .contains("corrompidos")
        );

        // A modified image fails the hash; a missing manifest or database is not a data file.
        let bad_image = tamper(&good, out.path(), |entries| {
            entries
                .iter_mut()
                .find(|(n, _)| n.starts_with("images/"))
                .unwrap()
                .1 = b"evil".to_vec();
        });
        assert!(import_into(&target, &bad_image).is_err());
        let no_manifest = tamper(&good, out.path(), |entries| {
            entries.retain(|(n, _)| n != MANIFEST_FILE);
        });
        assert!(import_into(&target, &no_manifest).is_err());
        let no_database = tamper(&good, out.path(), |entries| {
            entries.retain(|(n, _)| n != DATABASE_FILE);
        });
        assert!(import_into(&target, &no_database).is_err());

        // Paths that could escape, or that a snapshot never has, are refused.
        for name in [
            "../escape.txt",
            "images/../../escape.png",
            "/etc/passwd",
            "notes.txt",
        ] {
            let extra = tamper(&good, out.path(), |entries| {
                entries.push((name.to_owned(), b"x".to_vec()));
            });
            assert!(import_into(&target, &extra).is_err(), "{name}");
        }
        assert!(!out.path().join("escape.txt").exists());

        // A file from a newer version of the app.
        let newer = tamper(&good, out.path(), |entries| {
            let manifest = entries
                .iter_mut()
                .find(|(n, _)| n == MANIFEST_FILE)
                .unwrap();
            let mut value: serde_json::Value = serde_json::from_slice(&manifest.1).unwrap();
            value["schemaVersion"] = serde_json::json!(crate::db::SCHEMA_VERSION + 1);
            manifest.1 = serde_json::to_vec(&value).unwrap();
        });
        assert_eq!(
            import_into(&target, &newer).unwrap_err().to_string(),
            crate::db::NEWER_SCHEMA_MESSAGE
        );

        assert_eq!(questions(&target.db), before);
        assert!(
            list(&default_folder(&target.paths)).is_empty(),
            "no safety backup for a file that was rejected"
        );
        assert!(!target.paths.images_dir().join(IMAGE).exists());
    }

    #[test]
    fn a_damaged_archive_is_not_a_data_file_and_not_an_internal_error() {
        let source = profile();
        add_question(&source.db, "from-source");
        let out = tempfile::tempdir().unwrap();
        let good = out.path().join("good.gabaritei");
        export(&source.db, &source.paths, &good, NOW).unwrap();

        // Flip a byte inside the compressed data: the archive still opens, but reading an
        // entry fails (bad checksum or invalid compressed data).
        let mut bytes = fs::read(&good).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0xFF;
        let damaged = out.path().join("damaged.gabaritei");
        fs::write(&damaged, bytes).unwrap();

        let target = profile();
        add_question(&target.db, "local-only");
        let error = import_into(&target, &damaged).unwrap_err();
        assert_eq!(error.to_string(), NOT_A_DATA_FILE);
        assert_eq!(questions(&target.db).len(), 1);
    }

    #[test]
    fn size_limits_apply_per_file_and_in_total() {
        let source = profile();
        fs::write(source.paths.images_dir().join(IMAGE), vec![7u8; 1_000]).unwrap();
        fs::write(
            source
                .paths
                .images_dir()
                .join("0123456789abcdef01234568.png"),
            vec![8u8; 1_000],
        )
        .unwrap();
        let out = tempfile::tempdir().unwrap();
        let file = out.path().join("data.gabaritei");
        export(&source.db, &source.paths, &file, NOW).unwrap();
        // The size of the database copy inside the file (the live file may still be in the WAL).
        let probe = tempfile::tempdir().unwrap();
        unpack(&file, probe.path()).unwrap();
        let database_size = fs::metadata(probe.path().join(DATABASE_FILE))
            .unwrap()
            .len();
        let roomy = Limits {
            database: database_size * 4,
            image: 1_000,
            total: database_size * 4 + 10_000,
        };
        let unpack_with = |limits| {
            let into = tempfile::tempdir().unwrap();
            unpack_within(&file, into.path(), limits)
        };
        // An image exactly at its limit is fine; one byte over is not.
        assert!(unpack_with(roomy).is_ok());
        assert!(
            unpack_with(Limits {
                image: 999,
                ..roomy
            })
            .is_err()
        );
        assert!(
            unpack_with(Limits {
                database: 10,
                ..roomy
            })
            .is_err()
        );
        // Each file is within its own limit, but together they are too much.
        assert!(
            unpack_with(Limits {
                total: database_size + 1_500,
                ..roomy
            })
            .is_err()
        );
    }

    #[test]
    fn an_oversized_entry_is_rejected_while_unpacking() {
        // The cap itself is huge; check the mechanism with a name the snapshot never has.
        let out = tempfile::tempdir().unwrap();
        let path = out.path().join("odd.gabaritei");
        let mut writer = ZipWriter::new(File::create(&path).unwrap());
        writer
            .start_file("images/not-a-hash.png", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"x").unwrap();
        writer.finish().unwrap();
        let into = tempfile::tempdir().unwrap();
        assert!(unpack(&path, into.path()).is_err());
    }
}
