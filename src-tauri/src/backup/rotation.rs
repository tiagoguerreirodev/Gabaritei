use super::*;
use std::time::SystemTime;

/// Daily backups kept.
pub(super) const KEEP_BACKUPS: usize = 7;

/// Backups made by hand with "Fazer backup agora" kept. Apart from the daily ones, so pressing
/// the button never pushes a daily backup out.
pub(super) const KEEP_MANUAL: usize = 7;

/// Safety backups kept.
pub(super) const KEEP_SAFETY: usize = 5;

/// A new automatic backup is due when the newest one is older than this.
pub(super) const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Whether the newest backup of this computer is older than a day (or there is none).
/// A backup another computer made into a shared folder doesn't count. Read from the folder
/// names alone (the stamp has the time of the backup), so it stays cheap on a cloud-synced
/// folder whose manifests would have to be downloaded.
pub(super) fn is_due(folder: &Path, computer: &Computer, now: OffsetDateTime) -> bool {
    let cutoff = Stamp::at(now - INTERVAL);
    let present = Stamp::at(now);
    try_list_names(folder)
        .unwrap_or_default()
        .into_iter()
        .filter(|backup| backup.kind == BackupKind::Backup && !computer.is_other(backup))
        .map(|backup| backup.stamp)
        // A backup dated in the future (the clock was wrong once) must not stop new ones.
        .filter(|created| *created <= present)
        .max()
        .is_none_or(|newest| newest < cutoff)
}

/// Writes one backup into a temporary folder, publishes it, then prunes its kind.
pub(super) fn create(
    db: &Database,
    paths: &AppPaths,
    folder: &Path,
    kind: BackupKind,
    computer: &Computer,
    now: OffsetDateTime,
) -> AppResult<BackupInfo> {
    let pending = begin(folder, kind, computer, now)?;
    let manifest = snapshot::write(
        db,
        &paths.images_dir(),
        &pending.building,
        &crate::clock::format(now),
    )
    .inspect_err(|_| pending.abandon())?;
    let info = pending.finish(manifest)?;
    prune(folder, kind, computer, now.into());
    Ok(info)
}

/// A backup being built in a hidden temporary folder, not yet visible in the list.
pub(super) struct Pending {
    name: BackupName,
    pub(super) building: PathBuf,
    finished: PathBuf,
}

/// Creates the empty temporary folder for a backup of `kind` made at `now`.
pub(super) fn begin(
    folder: &Path,
    kind: BackupKind,
    computer: &Computer,
    now: OffsetDateTime,
) -> AppResult<Pending> {
    fs::create_dir_all(folder)?;
    let name = BackupName::new(kind, Stamp::at(now), computer);
    let finished = folder.join(&name.name);
    if finished.exists() {
        return Err(AppError::Invalid(
            "Já existe um backup deste instante. Tente de novo.".into(),
        ));
    }
    let building = folder.join(format!("{TEMP_PREFIX}{}", name.name));
    remove_dir_logged(&building);
    fs::create_dir(&building)?;
    Ok(Pending {
        name,
        building,
        finished,
    })
}

impl Pending {
    /// Removes the temporary folder after a failure.
    pub(super) fn abandon(&self) {
        remove_dir_logged(&self.building);
    }

    /// Publishes the finished backup.
    pub(super) fn finish(self, manifest: snapshot::Manifest) -> AppResult<BackupInfo> {
        fs::rename(&self.building, &self.finished).inspect_err(|_| self.abandon())?;
        Ok(BackupInfo {
            name: self.name.name,
            kind: self.name.kind,
            computer: self.name.tag,
            other_computer: false,
            size_bytes: manifest.size(),
            created_at: manifest.created_at,
        })
    }
}

/// Whether a temporary folder was last changed more than a day ago: no build takes that long.
fn is_stale(path: &Path, now: SystemTime) -> bool {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age > INTERVAL)
}

/// Deletes a folder tree, writing a failure to the app log. A backup's cleanup must not fail
/// the backup (a sync client can hold a file open), but a leftover that never goes away should
/// be findable. A folder that is already gone is fine.
pub(crate) fn remove_dir_logged(path: &Path) {
    if let Err(error) = fs::remove_dir_all(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        crate::log::error(format!("could not delete {}: {error}", path.display()));
    }
}

/// Deletes this computer's oldest backups of `kind` beyond the limit, and its temporary
/// folders left by a crash. Backups and temporary folders of another computer sharing the
/// folder are never touched, and neither are untagged ones from before tags existed (they
/// can't be told apart). A failure to delete is not worth failing the backup that just
/// succeeded.
pub(super) fn prune(folder: &Path, kind: BackupKind, computer: &Computer, now: SystemTime) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    let mut names: Vec<String> = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if let Some(building) = name.strip_prefix(TEMP_PREFIX) {
            // Another computer's in-progress build is left alone. This computer's own are
            // removed at once (it is the one running); untagged or unreadable ones, left by a
            // crash before tags existed, once they are too old to be in progress.
            let parsed = BackupName::parse(building);
            let theirs = parsed
                .as_ref()
                .is_some_and(|backup| computer.is_other(backup));
            let mine = parsed.as_ref().is_some_and(|backup| computer.owns(backup));
            if mine || (!theirs && is_stale(&entry.path(), now)) {
                remove_dir_logged(&entry.path());
            }
        } else if BackupName::parse(&name)
            .is_some_and(|backup| backup.kind == kind && computer.owns(&backup))
        {
            names.push(name);
        }
    }
    // The stamp is the same length for every name and comes first, so names sort by time.
    names.sort();
    let excess = names.len().saturating_sub(kind.keep());
    for name in &names[..excess] {
        remove_dir_logged(&folder.join(name));
    }
}

#[cfg(test)]
mod tests;
