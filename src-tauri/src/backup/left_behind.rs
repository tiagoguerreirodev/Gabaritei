use super::*;

/// The notice for the backups `left` in `old`, if there are any.
pub(super) fn left_behind(old: &Path, left: &[&ParsedName]) -> Option<LeftBehind> {
    (!left.is_empty()).then(|| LeftBehind {
        folder: old.display().to_string(),
        backups: left.len() as u32,
        safety: left
            .iter()
            .filter(|backup| backup.kind == BackupKind::Safety)
            .count() as u32,
        unavailable: false,
    })
}

/// Stores (or, with `None`, forgets) the old folder whose backups were left behind.
pub(super) fn remember_old_folder(conn: &Connection, old: Option<&Path>) -> AppResult<()> {
    match old {
        Some(old) => set_setting(
            conn,
            SettingKey::BackupLeftBehind,
            &old.display().to_string(),
        ),
        None => crate::db::put_raw_setting(conn, SettingKey::BackupLeftBehind, None),
    }
}

/// What looking at the remembered old folder found.
pub(super) enum OldFolder {
    /// It is the current folder now.
    Current,
    /// Its backups, by name.
    Listed(Vec<ParsedName>),
    /// It can't be read (gone, unplugged, no permission).
    Unreadable,
}

/// Looks at the old folder: whether it is the current one, and what it holds.
pub(super) fn inspect_old_folder(old: &Path, current: &Path) -> OldFolder {
    if same_folder(old, current) {
        OldFolder::Current
    } else {
        try_list_names(old).map_or(OldFolder::Unreadable, OldFolder::Listed)
    }
}

/// The remembered old folder and what it still holds of this computer's, when there is
/// something to offer. A stale memory (the folder is the current one now, or has nothing of ours
/// left) is forgotten here. An unreadable folder stays remembered and is offered as
/// `unavailable`, so it can be dismissed. Also returns every backup name found there (empty
/// when unavailable), so a move works from what the notice counted and reads the folder once.
pub(super) fn pending_old_folder(
    db: &Database,
    probe: &FolderProbe,
    current: &Path,
) -> AppResult<Option<(PathBuf, LeftBehind, Vec<ParsedName>)>> {
    let Some(old) = db.with(|conn| get_setting::<String>(conn, SettingKey::BackupLeftBehind))?
    else {
        return Ok(None);
    };
    let old = PathBuf::from(old);
    let unavailable = || LeftBehind {
        folder: old.display().to_string(),
        backups: 0,
        safety: 0,
        unavailable: true,
    };
    let (notice, names) = match probe.inspect(&old, current) {
        Some(OldFolder::Current) => (None, Vec::new()),
        Some(OldFolder::Listed(found)) => {
            let mine: Vec<&ParsedName> =
                found.iter().filter(|backup| !leave_alone(backup)).collect();
            (left_behind(&old, &mine), found)
        }
        // Not answering in time is the same as unreadable: the drive may come back.
        Some(OldFolder::Unreadable) | None => (Some(unavailable()), Vec::new()),
    };
    if notice.is_none() {
        forget_old_folder_if(db, &old)?;
    }
    Ok(notice.map(|notice| (old, notice, names)))
}

/// Forgets the remembered old folder only if it is still `old`. `status` runs without the backup
/// lock, so a folder change could have stored a newer one since it was read; that one stays.
pub(super) fn forget_old_folder_if(db: &Database, old: &Path) -> AppResult<()> {
    db.with(|conn| {
        let stored = get_setting::<String>(conn, SettingKey::BackupLeftBehind)?;
        if stored.as_deref() == Some(old.display().to_string().as_str()) {
            remember_old_folder(conn, None)?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests;
