use super::*;

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
pub(super) fn is_due(folder: &Path, now: OffsetDateTime) -> bool {
    let cutoff = crate::clock::format(now - INTERVAL);
    let present = crate::clock::format(now);
    try_list_names(folder)
        .unwrap_or_default()
        .iter()
        .filter(|backup| {
            backup.kind == BackupKind::Backup
                && !is_other_computer(backup.tag.as_deref(), &backup.stamp)
        })
        .map(|backup| stamp_to_iso(&backup.stamp))
        // A backup dated in the future (the clock was wrong once) must not stop new ones.
        .filter(|created| *created <= present)
        .max()
        .is_none_or(|newest| newest < cutoff)
}

/// Writes one backup into `folder`: into a hidden temporary folder first, then renamed.
pub(super) fn create(
    db: &Database,
    paths: &AppPaths,
    folder: &Path,
    kind: BackupKind,
    now: &str,
    prune: bool,
) -> AppResult<BackupInfo> {
    create_with(
        |building| snapshot::write(db, &paths.images_dir(), building, now),
        folder,
        kind,
        now,
        prune,
    )
}

/// [`create`] with the snapshot written by `write_snapshot` into the folder it is given.
pub(super) fn create_with(
    write_snapshot: impl FnOnce(&Path) -> AppResult<snapshot::Manifest>,
    folder: &Path,
    kind: BackupKind,
    now: &str,
    prune: bool,
) -> AppResult<BackupInfo> {
    let pending = begin(folder, kind, now)?;
    let manifest = write_snapshot(&pending.building).inspect_err(|_| pending.abandon())?;
    pending.finish(folder, kind, manifest, prune)
}

/// A backup being built in a hidden temporary folder, not yet visible in the list.
pub(super) struct Pending {
    pub(super) name: String,
    pub(super) computer: Option<String>,
    pub(super) building: PathBuf,
    pub(super) finished: PathBuf,
}

/// Creates the empty temporary folder for a backup of `kind` made at `now`.
pub(super) fn begin(folder: &Path, kind: BackupKind, now: &str) -> AppResult<Pending> {
    fs::create_dir_all(folder)?;
    let name = match computer_tag() {
        Some(tag) => format!("{}-{}-{tag}", kind.prefix(), stamp(now)),
        None => format!("{}-{}", kind.prefix(), stamp(now)),
    };
    let finished = folder.join(&name);
    if finished.exists() {
        return Err(AppError::Invalid(
            "Já existe um backup deste instante. Tente de novo.".into(),
        ));
    }
    let building = folder.join(format!(".tmp-{name}"));
    remove_dir_logged(&building);
    fs::create_dir(&building)?;
    Ok(Pending {
        name,
        computer: computer_tag(),
        building,
        finished,
    })
}

impl Pending {
    /// Removes the temporary folder after a failure.
    pub(super) fn abandon(&self) {
        remove_dir_logged(&self.building);
    }

    /// Renames the temporary folder to its final name (the backup now exists) and, if asked,
    /// prunes the old backups of its kind.
    pub(super) fn finish(
        self,
        folder: &Path,
        kind: BackupKind,
        manifest: snapshot::Manifest,
        prune: bool,
    ) -> AppResult<BackupInfo> {
        fs::rename(&self.building, &self.finished).inspect_err(|_| self.abandon())?;
        if prune {
            self::prune(folder, kind);
        }
        Ok(BackupInfo {
            name: self.name,
            kind,
            computer: self.computer,
            other_computer: false,
            size_bytes: manifest.size(),
            created_at: manifest.created_at,
        })
    }
}

/// Whether a temporary folder was last changed more than a day ago: no build takes that long.
pub(super) fn is_stale(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
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
pub(super) fn prune(folder: &Path, kind: BackupKind) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    let mut names: Vec<String> = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if let Some(building) = name.strip_prefix(".tmp-") {
            // Another computer's in-progress build is left alone. This computer's own are
            // removed at once (it is the one running); untagged or unreadable ones, left by a
            // crash before tags existed, once they are too old to be in progress.
            let parsed = parse(building);
            let theirs = parsed
                .as_ref()
                .is_some_and(|parsed| is_other_computer(parsed.tag.as_deref(), &parsed.stamp));
            let mine = parsed.is_some_and(|parsed| owns_tag(parsed.tag.as_deref(), &parsed.stamp));
            if mine || (!theirs && is_stale(&entry.path())) {
                remove_dir_logged(&entry.path());
            }
        } else if parse(&name).is_some_and(|parsed| {
            parsed.kind == kind && owns_tag(parsed.tag.as_deref(), &parsed.stamp)
        }) {
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
