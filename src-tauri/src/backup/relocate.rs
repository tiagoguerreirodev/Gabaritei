//! Copying or moving the backups left in the old folder to the new one, after the backup
//! folder changed.
//!
//! Each backup is copied (only its manifest and the files the manifest lists) into a hidden
//! temporary folder in the new folder, flushed to disk, checked with [`snapshot::verify`]
//! (every hash, the database's integrity and schema) and renamed to its name. The original is
//! removed only after that, and only when the user asked to move: it is first renamed to a
//! hidden name (atomic, so it never shows as a half-deleted backup) and then deleted. Any
//! failure before that leaves the original untouched.

#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::path::Path;

use serde::Serialize;
use ts_rs::TS;

use super::snapshot::{self, Manifest};
use crate::error::{AppError, AppResult};

/// What a copy or move did, in backups.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RelocateReport {
    /// Backups now in the new folder that were copied there.
    pub copied: u32,
    /// Backups that were already in the new folder, identical to the original.
    pub already_there: u32,
    /// Originals deleted after their copy was verified (when moving).
    pub removed: u32,
    /// Backups that could not be copied and verified, or whose original could not be removed.
    /// A failed copy leaves its original untouched.
    pub failed: u32,
    /// Why the first failed one failed (pt-BR, with the backup's name).
    pub first_failure: Option<String>,
    /// Backups of another computer (or untagged ones on a tagged computer) in the old folder,
    /// which are never copied or moved: that computer may still use the folder.
    pub left_alone: u32,
}

/// Runs one backup's work in its own turn. The caller takes whatever lock keeps backups,
/// restores and imports apart for the length of `work`, and may skip it by not calling it
/// (the folder changed meanwhile).
pub type Turn<'a> = &'a dyn Fn(&mut dyn FnMut());

/// Copies the backups called `names` from `old` to `new` (which must exist: the caller creates it
/// under its lock) and, when `remove_originals`, removes each original once its copy is
/// verified. Each backup runs inside `turn`, so the lock is held
/// per backup and not for the whole copy; a turn that does not run `work` stops the rest.
pub fn relocate(
    old: &Path,
    new: &Path,
    names: &[String],
    remove_originals: bool,
    turn: Turn<'_>,
) -> AppResult<RelocateReport> {
    let mut report = RelocateReport::default();
    for (index, name) in names.iter().enumerate() {
        let mut result = None;
        turn(&mut || result = Some(relocate_one(old, new, name, remove_originals)));
        let Some(result) = result else {
            // Not run: the folder changed or something else needed the turn for good.
            let skipped = (names.len() - index) as u32;
            report.failed += skipped;
            report.first_failure.get_or_insert_with(|| {
                "A cópia parou porque a pasta dos backups mudou ou a oferta foi descartada; os demais ficaram na pasta anterior."
                    .to_owned()
            });
            break;
        };
        match result {
            Ok(Done { copied, removed }) => {
                if copied {
                    report.copied += 1;
                } else {
                    report.already_there += 1;
                }
                report.removed += u32::from(removed);
            }
            Err(error) => {
                crate::log::error(format!(
                    "could not {} backup {name}: {}",
                    if remove_originals { "move" } else { "copy" },
                    error.detail()
                ));
                report.failed += 1;
                report
                    .first_failure
                    .get_or_insert_with(|| format!("{name}: {error}"));
            }
        }
    }
    Ok(report)
}

struct Done {
    copied: bool,
    removed: bool,
}

fn relocate_one(old: &Path, new: &Path, name: &str, remove: bool) -> AppResult<Done> {
    let source = old.join(name);
    let target = new.join(name);
    let original = snapshot::read_manifest(&source)?;
    let copied = if target.exists() {
        // Never delete the only copy: "already there" must be a different folder, however the
        // two paths are spelled (another mount of the same share, a link, letter case).
        if same_file::is_same_file(&source, &target).unwrap_or(true) {
            return Err(AppError::Invalid(
                "A pasta nova e a anterior parecem ser a mesma pasta.".into(),
            ));
        }
        // The same backup (a copy made before): fine only if complete and exactly the same.
        let existing = snapshot::verify(&target)?;
        if existing != original {
            return Err(AppError::Invalid(
                "Já existe outro backup com este nome na pasta nova.".into(),
            ));
        }
        false
    } else {
        let building = new.join(format!(".tmp-{name}"));
        super::remove_dir_logged(&building);
        let made = copy_snapshot(&source, &original, &building)
            .and_then(|()| snapshot::verify(&building))
            .and_then(|copy| {
                if copy == original {
                    fs::rename(&building, &target).map_err(Into::into)
                } else {
                    Err(AppError::Invalid(
                        "A cópia não confere com o original.".into(),
                    ))
                }
            });
        if let Err(error) = made {
            super::remove_dir_logged(&building);
            return Err(error);
        }
        sync_dir(new);
        true
    };
    // Only reached with a verified copy in place.
    if remove {
        remove_original(old, name, &source)?;
    }
    Ok(Done {
        copied,
        removed: remove,
    })
}

/// Takes the original out of the folder in one step (a rename), then deletes it. Failing to
/// delete afterwards (a sync client holding a file) leaves only a hidden `.tmp-` leftover, never
/// a half-deleted backup in the list. The leftover is removed when this runs again for the same
/// name, or by the backup cleanup if that folder becomes the backup folder again.
fn remove_original(old: &Path, name: &str, source: &Path) -> AppResult<()> {
    let aside = old.join(format!(".tmp-{name}"));
    super::remove_dir_logged(&aside);
    fs::rename(source, &aside)?;
    super::remove_dir_logged(&aside);
    Ok(())
}

/// Copies `manifest.json` and the files it lists, nothing else, flushing each to disk so a
/// power cut right after a move cannot leave only an incomplete copy.
fn copy_snapshot(source: &Path, manifest: &Manifest, dest: &Path) -> AppResult<()> {
    fs::create_dir_all(dest)?;
    let mut files = vec![snapshot::MANIFEST_FILE];
    for file in &manifest.files {
        if !snapshot::is_allowed_path(&file.path) {
            return Err(AppError::Invalid(snapshot::CORRUPT_MESSAGE.into()));
        }
        files.push(file.path.as_str());
    }
    for relative in files {
        let from = source.join(relative);
        // `file_type` of the entry itself: a link in a backup is never followed.
        if !fs::symlink_metadata(&from)?.file_type().is_file() {
            return Err(AppError::Invalid(snapshot::CORRUPT_MESSAGE.into()));
        }
        let to = dest.join(relative);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&from, &to)?;
        // Opened for writing: Windows refuses to flush a read-only handle.
        OpenOptions::new().write(true).open(&to)?.sync_all()?;
    }
    sync_dir(dest);
    Ok(())
}

/// Flushes a folder's entries to disk where the OS allows it (not on Windows).
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(handle) = File::open(dir) {
        let _ = handle.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}
