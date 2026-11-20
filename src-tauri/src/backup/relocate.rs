//! Copying or moving the backups left in the old folder to the new one, after the backup
//! folder changed.
//!
//! Each backup is copied into a hidden temporary folder in the new folder, checked with
//! [`snapshot::verify`] (every hash, the database's integrity and schema), and only then renamed
//! to its name. The original is deleted only after that, and only when the user asked to move.
//! Any failure leaves the original where it was.

use std::fs;
use std::path::Path;

use serde::Serialize;
use ts_rs::TS;

use super::snapshot;
use crate::error::AppResult;

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
    /// Backups that could not be copied and verified, or whose original could not be deleted.
    /// Their originals are untouched or, at worst, still complete.
    pub failed: u32,
    /// Why the first failed one failed (pt-BR, with the backup's name).
    pub first_failure: Option<String>,
}

/// Copies every complete backup in `old` to `new` and, when `remove_originals`, deletes each
/// original once its copy is verified. `names` are the backups to handle (see [`super::list`]).
pub fn relocate(
    old: &Path,
    new: &Path,
    names: &[String],
    remove_originals: bool,
) -> AppResult<RelocateReport> {
    fs::create_dir_all(new)?;
    let mut report = RelocateReport::default();
    for name in names {
        match relocate_one(&old.join(name), new, name, remove_originals) {
            Ok(Done { copied, removed }) => {
                if copied {
                    report.copied += 1;
                } else {
                    report.already_there += 1;
                }
                report.removed += u32::from(removed);
            }
            Err(error) => {
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

fn relocate_one(source: &Path, new: &Path, name: &str, remove: bool) -> AppResult<Done> {
    let target = new.join(name);
    let original = snapshot::read_manifest(source)?;
    let copied = if target.exists() {
        // The same backup (a folder shared by both, or a copy made before): fine only if it
        // is complete and says exactly the same.
        let existing = snapshot::verify(&target)?;
        if existing != original {
            return Err(crate::error::AppError::Invalid(
                "Já existe outro backup com este nome na pasta nova.".into(),
            ));
        }
        false
    } else {
        let building = new.join(format!(".tmp-{name}"));
        let _ = fs::remove_dir_all(&building);
        let made = copy_dir(source, &building)
            .and_then(|()| snapshot::verify(&building))
            .and_then(|copy| {
                if copy == original {
                    fs::rename(&building, &target).map_err(Into::into)
                } else {
                    Err(crate::error::AppError::Invalid(
                        "A cópia não confere com o original.".into(),
                    ))
                }
            });
        if let Err(error) = made {
            let _ = fs::remove_dir_all(&building);
            return Err(error);
        }
        true
    };
    // Only reached with a verified copy in place.
    let removed = remove && fs::remove_dir_all(source).is_ok();
    if remove && !removed {
        return Err(crate::error::AppError::Invalid(
            "O original não pôde ser apagado.".into(),
        ));
    }
    Ok(Done { copied, removed })
}

/// Copies a folder of regular files and folders. Anything else (a link, say) is an error,
/// since a backup never holds one.
fn copy_dir(from: &Path, to: &Path) -> AppResult<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target)?;
        } else {
            return Err(crate::error::AppError::Invalid(
                "O backup tem um item que não é um arquivo.".into(),
            ));
        }
    }
    Ok(())
}
