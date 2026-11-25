use super::*;

/// Replaces the current data with the backup called `name`:
///
/// 1. checks the backup completely (hashes, `integrity_check`, schema version): a bad backup
///    stops here and nothing changes, not even a safety backup;
/// 2. makes a safety backup of the current data;
/// 3. replaces the database and adds the images.
///
/// This computer's own settings (backup folder, CLI paths) are kept.
pub(super) fn restore(
    db: &Database,
    paths: &AppPaths,
    folder: &Path,
    name: &str,
    now: &str,
    cleanup: RestoreCleanup,
) -> AppResult<()> {
    if parse_name(name).is_none() {
        return Err(AppError::Invalid("Backup não encontrado.".into()));
    }
    let source = folder.join(name);
    if !source.is_dir() {
        return Err(AppError::Invalid("Backup não encontrado.".into()));
    }
    replace_data(db, paths, folder, &source, now, cleanup)
}

/// Replaces the data with the snapshot in `source`: checks and stages it (a bad snapshot stops
/// here and nothing changes, not even a safety backup), makes a safety backup of the current
/// data, then replaces it. Shared with the import of a data file.
pub(super) fn replace_data(
    db: &Database,
    paths: &AppPaths,
    folder: &Path,
    source: &Path,
    now: &str,
    cleanup: RestoreCleanup,
) -> AppResult<()> {
    let staged = snapshot::stage(source)?;
    // A replacement that SQLite would refuse is stopped here, before any image is added or a
    // safety backup is made. The lock is only held for this quick look.
    db.with(|conn| snapshot::ensure_replaceable(conn, &staged))?;
    // Adding images is harmless to the current data, so it stays out of the lock.
    snapshot::copy_images(&paths.images_dir(), source, &staged)?;
    // The safety backup's own images are copied before the lock too (they are append-only), so
    // a large image folder never freezes the other commands. Only the database copy, the images
    // added since, and the replacement run under the lock.
    let pending = begin(folder, BackupKind::Safety, now)?;
    let ahead = snapshot::copy_images_ahead(&paths.images_dir(), &pending.building)
        .inspect_err(|_| pending.abandon())?;
    // One lock around the safety backup and the replacement: a write in between would be in
    // neither.
    db.with(|conn| {
        let manifest =
            snapshot::write_locked(conn, &paths.images_dir(), &pending.building, now, ahead)
                .inspect_err(|_| pending.abandon())?;
        // Pruned only afterwards, so a safety backup being restored is never deleted by this one.
        pending.finish(folder, BackupKind::Safety, manifest, false)?;
        snapshot::commit(conn, &staged, LOCAL_SETTINGS, cleanup)
    })?;
    prune(folder, BackupKind::Safety);
    Ok(())
}

#[cfg(test)]
mod tests;
