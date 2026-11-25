use super::*;

/// Runs backup, restore and import one at a time, and remembers why the last backup failed.
/// Managed as Tauri state.
#[derive(Default)]
pub struct BackupService {
    pub(super) busy: Mutex<()>,
    pub(super) last_error: Mutex<Option<String>>,
    /// A copy or move of the old folder's backups is running. Its turns share the `busy` lock
    /// with everything else, so a second one would interleave with it and report failures that
    /// are not real.
    pub(super) relocating: AtomicBool,
    /// Bounds filesystem checks that may touch an offline share.
    pub(super) probe: FolderProbe,
}

/// Marks a relocation as running until dropped.
pub(super) struct Relocating<'a>(&'a AtomicBool);

impl<'a> Relocating<'a> {
    fn start(flag: &'a AtomicBool) -> AppResult<Self> {
        if flag.swap(true, Ordering::AcqRel) {
            return Err(AppError::Invalid(
                "Já há uma cópia de backups em andamento. Aguarde ela terminar.".into(),
            ));
        }
        Ok(Self(flag))
    }
}

impl Drop for Relocating<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl BackupService {
    /// Runs `work` once nothing else is backing up, restoring or importing.
    pub fn exclusive<T>(&self, work: impl FnOnce() -> T) -> T {
        let _turn = lock(&self.busy);
        work()
    }

    pub(super) fn remember<T>(&self, result: &AppResult<T>) {
        *lock(&self.last_error) = result.as_ref().err().map(ToString::to_string);
    }

    /// Whether a backup, restore or import is running now.
    pub fn is_busy(&self) -> bool {
        self.busy
            .try_lock()
            .is_err_and(|error| matches!(error, std::sync::TryLockError::WouldBlock))
    }

    /// Fails with a clear message when the backup folder does not answer within the deadline,
    /// before a backup, restore or import takes the lock for real work in it. A folder that does
    /// not exist yet answers at once (with an error) and is fine: it is created on first use.
    fn ensure_reachable(&self, folder: &Path) -> AppResult<()> {
        let path = folder.to_path_buf();
        match self.probe.run(folder, move || folder_state(&path)) {
            Some(FolderState::Present | FolderState::Missing) => Ok(()),
            Some(FolderState::Unavailable) | None => Err(folder_unavailable()),
        }
    }

    /// The *Dados* section's data.
    pub fn status(&self, db: &Database, paths: &AppPaths) -> AppResult<BackupStatus> {
        let (folder, is_default_folder) = db.with(|conn| {
            let chosen = get_setting::<String>(conn, SettingKey::BackupFolder)?;
            Ok((folder(conn, paths)?, chosen.is_none()))
        })?;
        let left_behind =
            pending_old_folder(db, &self.probe, &folder)?.map(|(_, notice, _)| notice);
        let listing = self.listing(&folder);
        Ok(BackupStatus {
            folder: folder.display().to_string(),
            is_default_folder,
            last_error: lock(&self.last_error).clone(),
            left_behind,
            listing,
        })
    }

    fn listing(&self, folder: &Path) -> BackupListing {
        let path = folder.to_path_buf();
        match self.probe.run(folder, move || folder_state(&path)) {
            Some(FolderState::Present) => {
                let path = folder.to_path_buf();
                // Use a separate slot so a slow manifest read cannot hold up the quick checks
                // for backups, restores or imports.
                match self
                    .probe
                    .run_for(&manifest_key(folder), MANIFEST_READ_TIMEOUT, move || {
                        try_list(&path)
                    }) {
                    Some(Some(backups)) => BackupListing::Ready { backups },
                    Some(None) => BackupListing::FolderUnavailable,
                    None => BackupListing::Slow,
                }
            }
            Some(FolderState::Missing) => BackupListing::Ready {
                backups: Vec::new(),
            },
            Some(FolderState::Unavailable) | None => BackupListing::FolderUnavailable,
        }
    }

    /// Forgets the offer to move backups from the old folder. Nothing is moved or deleted.
    pub fn forget_left_behind(&self, db: &Database) -> AppResult<()> {
        self.exclusive(|| db.with(|conn| remember_old_folder(conn, None)))
    }

    /// Changes the backup folder (`None` goes back to the default). The answer says what stays
    /// behind in the old folder: nothing is moved or deleted there.
    pub fn change_folder(
        &self,
        db: &Database,
        paths: &AppPaths,
        chosen: Option<&Path>,
    ) -> AppResult<BackupStatus> {
        self.change_folder_with(db, paths, chosen, check_chosen_folder)
    }

    pub(super) fn change_folder_with(
        &self,
        db: &Database,
        paths: &AppPaths,
        chosen: Option<&Path>,
        check: impl FnOnce(&Path, &Path) -> AppResult<String> + Send + 'static,
    ) -> AppResult<BackupStatus> {
        let checked = chosen
            .map(|chosen| {
                let (candidate, images_dir) = (chosen.to_path_buf(), paths.images_dir());
                self.probe
                    .run(&candidate_key(chosen), move || {
                        check(&candidate, &images_dir)
                    })
                    .ok_or_else(folder_unavailable)?
            })
            .transpose()?;
        let new = checked
            .as_deref()
            .map_or_else(|| default_folder(paths), PathBuf::from);

        // Not while a backup is being written: it could land in the old folder after the
        // count below. Read the current folder only after taking the lock, since it may have
        // changed while the chosen folder was being checked.
        self.exclusive(|| {
            let old = db.with(|conn| folder(conn, paths))?;
            let has_backups = match self.probe.inspect(&old, &new) {
                Some(OldFolder::Listed(found)) => found.iter().any(|b| !leave_alone(b)),
                Some(OldFolder::Current | OldFolder::Unreadable) => false,
                // No answer in time (an offline share): remember it, offered as unavailable.
                None => true,
            };
            db.transaction(|tx| {
                match &checked {
                    Some(folder) => set_setting(tx, SettingKey::BackupFolder, folder)?,
                    None => crate::db::put_raw_setting(tx, SettingKey::BackupFolder, None)?,
                }
                remember_old_folder(tx, has_backups.then_some(old.as_path()))
            })?;
            self.status(db, paths)
        })
    }

    /// Copies the backups left in the old folder (see [`Self::change_folder`]) to the current
    /// one, or moves them when `remove_originals`: each original is deleted only after its copy
    /// passed [`snapshot::verify`]. Backups already in the new folder with the same name and
    /// content are not copied again.
    pub fn relocate_left_behind(
        &self,
        db: &Database,
        paths: &AppPaths,
        remove_originals: bool,
    ) -> AppResult<RelocateOutcome> {
        self.relocate_with(db, paths, remove_originals, &|| {})
    }

    /// [`Self::relocate_left_behind`] with `between` called after each backup's turn, once the
    /// lock is released. Tests use it to change things between two backups.
    pub(super) fn relocate_with(
        &self,
        db: &Database,
        paths: &AppPaths,
        remove_originals: bool,
        between: &dyn Fn(),
    ) -> AppResult<RelocateOutcome> {
        // One relocation at a time: the turns below take the lock per backup, so two calls
        // would interleave.
        let _relocating = Relocating::start(&self.relocating)?;
        // The lock is taken per backup, not for the whole copy: a scheduled backup can run between
        // two of them, and the folders are read again each time.
        let (old, new, names, others) = self.exclusive(|| {
            let nothing = || AppError::Invalid("Não há backups na pasta anterior.".into());
            let new = db.with(|conn| folder(conn, paths))?;
            // Read from the stored setting, so it works after a restart too, and never from a
            // path the frontend sent. It is cleared when the old folder has nothing of ours left.
            let (old, notice, found) =
                pending_old_folder(db, &self.probe, &new)?.ok_or_else(nothing)?;
            if notice.unavailable {
                return Err(AppError::Invalid(
                    "A pasta anterior não está acessível agora. Conecte-a de novo ou dispense o aviso."
                        .into(),
                ));
            }
            // A new folder inside the old one (so inside a backup, or a temporary folder) would
            // be deleted along with the original on a move. Looked at within the deadline: it
            // touches the old folder.
            let problem = {
                let (old_path, new_path) = (old.clone(), new.clone());
                self.probe
                    .run(&old, move || new_folder_problem(&old_path, &new_path))
            }
            .ok_or_else(|| {
                AppError::Invalid(
                    "A pasta anterior não está acessível agora. Conecte-a de novo ou dispense o aviso."
                        .into(),
                )
            })?;
            if let Some(message) = problem {
                return Err(AppError::Invalid(message.into()));
            }
            // Created here, under the lock, and not by the copy: the folder could have been
            // removed since it was chosen (an unplugged drive).
            self.ensure_reachable(&new)?;
            fs::create_dir_all(&new)?;
            // Another computer's backups, and untagged ones on a tagged computer, stay where they are:
            // they may belong to a computer that still uses that folder.
            let (others, mine): (Vec<_>, Vec<_>) = found.into_iter().partition(leave_alone);
            let names: Vec<String> = mine.into_iter().map(|backup| backup.name).collect();
            Ok((old, new, names, others.len() as u32))
        })?;
        let still_the_same = || {
            let current = db.with(|conn| {
                Ok((
                    folder(conn, paths)?,
                    get_setting::<String>(conn, SettingKey::BackupLeftBehind)?,
                ))
            });
            matches!(current, Ok((folder, Some(stored)))
                if same_folder(&folder, &new) && stored == old.display().to_string())
        };
        let mut report = relocate::relocate(&old, &new, &names, remove_originals, &|work| {
            self.exclusive(|| {
                if still_the_same() {
                    work();
                }
            });
            between();
        })?;
        report.left_alone = others;
        self.exclusive(|| {
            // After a move, what is still here is what could not be moved (the other computer's
            // backups are reported apart). A copy leaves the originals on purpose, unless it failed
            // for some backups: then the offer stays so the rest can be tried again. If the folder
            // changed meanwhile, the new change already decided what to remember.
            if still_the_same() {
                // A folder that stops answering (or can't be read) may still hold what is left:
                // keep the offer, shown as unavailable, rather than forgetting it.
                let remaining = (remove_originals || report.failed > 0)
                    && match self.probe.inspect(&old, &new) {
                        Some(OldFolder::Listed(found)) => found.iter().any(|b| !leave_alone(b)),
                        Some(OldFolder::Current) => false,
                        Some(OldFolder::Unreadable) | None => true,
                    };
                db.with(|conn| remember_old_folder(conn, remaining.then_some(old.as_path())))?;
            }
            let status = self.status(db, paths)?;
            Ok(RelocateOutcome { report, status })
        })
    }

    /// Makes a backup now.
    pub fn backup_now(&self, db: &Database, paths: &AppPaths, now: &str) -> AppResult<BackupInfo> {
        self.exclusive(|| {
            let result = db.with(|conn| folder(conn, paths)).and_then(|folder| {
                self.ensure_reachable(&folder)?;
                create(db, paths, &folder, BackupKind::Manual, now, true)
            });
            self.remember(&result);
            result
        })
    }

    /// Makes a backup if the newest one is older than a day. Called at start and every few minutes.
    pub fn backup_if_due(
        &self,
        db: &Database,
        paths: &AppPaths,
        now: OffsetDateTime,
    ) -> AppResult<Option<BackupInfo>> {
        self.exclusive(|| {
            let result = db.with(|conn| folder(conn, paths)).and_then(|folder| {
                // Bounded too, so a folder that doesn't answer is refused instead of held under the
                // lock. This runs in the background, so it may wait longer than a click would
                // (a drive spinning up); a gone drive is refused like everywhere else.
                let due = {
                    let path = folder.clone();
                    self.probe.run_for(&folder, MANIFEST_READ_TIMEOUT, move || {
                        match folder_state(&path) {
                            FolderState::Unavailable => None,
                            FolderState::Present | FolderState::Missing => Some(is_due(&path, now)),
                        }
                    })
                }
                .flatten()
                .ok_or_else(folder_unavailable)?;
                if due {
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
            self.ensure_reachable(&folder)?;
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
            let folder = db.with(|conn| folder(conn, paths))?;
            self.ensure_reachable(&folder)?;
            restore(db, paths, name, now)
        })
    }
}

#[cfg(test)]
mod tests;
