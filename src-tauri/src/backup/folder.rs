use super::*;

pub(super) const MAX_FOLDER: usize = 1_000;

/// What a quick look at the backup folder found.
pub(super) enum FolderState {
    /// It can be listed.
    Present,
    /// It does not exist yet (or any more) but its parent does: nothing wrong, it is created
    /// on first use.
    Missing,
    /// It can't be read, or its parent is gone too (a removed drive or volume).
    Unavailable,
}

pub(super) fn folder_state(folder: &Path) -> FolderState {
    match fs::read_dir(folder) {
        Ok(_) => FolderState::Present,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                // A root (`E:\`, `/`) that is missing is a gone drive, not a new folder.
                && folder.parent().is_some_and(Path::exists)
                && !mount_point_missing(folder) =>
        {
            FolderState::Missing
        }
        Err(_) => FolderState::Unavailable,
    }
}

/// macOS mounts drives and shares at `/Volumes/<name>`: a folder under a name that isn't there is
/// on a volume that is gone, not a folder that was never created.
#[cfg(target_os = "macos")]
pub(super) fn mount_point_missing(folder: &Path) -> bool {
    folder
        .strip_prefix("/Volumes")
        .ok()
        .and_then(|rest| rest.components().next())
        .is_some_and(|name| !Path::new("/Volumes").join(name).exists())
}

#[cfg(not(target_os = "macos"))]
pub(super) fn mount_point_missing(_folder: &Path) -> bool {
    false
}

pub(super) fn folder_unavailable() -> AppError {
    AppError::Invalid(
        "A pasta dos backups não está acessível agora (um disco desconectado ou uma pasta de rede fora do ar). Conecte-a de novo ou escolha outra pasta."
            .into(),
    )
}

/// The default backup folder.
pub fn default_folder(paths: &AppPaths) -> PathBuf {
    paths.data_dir().join("backups")
}

/// The folder to show the user for `which`. It is created if it doesn't exist yet (the backup
/// folder has nothing in it until the first backup), so opening it never fails for that.
pub fn folder_to_open(
    db: &Database,
    paths: &AppPaths,
    which: OpenableFolder,
) -> AppResult<PathBuf> {
    let path = match which {
        OpenableFolder::Data => paths.data_dir().to_path_buf(),
        OpenableFolder::Backups => db.with(|conn| folder(conn, paths))?,
    };
    fs::create_dir_all(&path)?;
    Ok(path)
}

/// The folder backups go to: the user's choice, or the default.
pub fn folder(conn: &Connection, paths: &AppPaths) -> AppResult<PathBuf> {
    Ok(get_setting::<String>(conn, SettingKey::BackupFolder)?
        .map(PathBuf::from)
        .unwrap_or_else(|| default_folder(paths)))
}

/// Checks that the chosen folder can be used and returns the path to store.
pub(super) fn check_chosen_folder(chosen: &Path, images_dir: &Path) -> AppResult<String> {
    let text = chosen
        .to_str()
        .filter(|text| !text.trim().is_empty() && text.chars().count() <= MAX_FOLDER)
        .ok_or_else(|| AppError::Invalid("Escolha uma pasta válida para os backups.".into()))?;
    if !chosen.is_absolute() {
        return Err(AppError::Invalid(
            "Escolha uma pasta válida para os backups.".into(),
        ));
    }
    if chosen.starts_with(images_dir) {
        return Err(AppError::Invalid(
            "Escolha uma pasta fora da pasta de imagens do Gabaritei.".into(),
        ));
    }
    // Inside a backup (or its hidden build folder) rotation could later delete the chosen folder.
    let inside_a_backup = |path: &Path| {
        path.components().any(|part| {
            part.as_os_str().to_str().is_some_and(|name| {
                name.starts_with(TEMP_PREFIX) || BackupName::parse(name).is_some()
            })
        })
    };
    let refuse =
        || AppError::Invalid("Escolha uma pasta que não esteja dentro de um backup.".into());
    if inside_a_backup(chosen) {
        return Err(refuse());
    }
    let not_writable = || {
        AppError::Invalid("Não foi possível gravar na pasta escolhida. Escolha outra pasta.".into())
    };
    // A link (or a Windows junction) can lead into a backup; look at where it really points.
    // Only a part of the path that already exists can be a link, so the deepest existing
    // ancestor is checked before anything is created: refusing then leaves no empty folders
    // inside the backup. `symlink_metadata` does not follow links, so a link whose target does
    // not exist yet is found too; where it would lead can't be checked, so it is refused.
    if let Some(existing) = chosen
        .ancestors()
        .find(|ancestor| ancestor.symlink_metadata().is_ok())
    {
        match fs::canonicalize(existing) {
            Ok(real) if inside_a_backup(&real) => return Err(refuse()),
            Ok(_) => {}
            Err(_) => return Err(not_writable()),
        }
    }
    fs::create_dir_all(chosen).map_err(|_| not_writable())?;
    // Checked again once it exists, in case the path changed meanwhile.
    if fs::canonicalize(chosen).is_ok_and(|real| inside_a_backup(&real)) {
        return Err(refuse());
    }
    tempfile::tempfile_in(chosen).map_err(|_| not_writable())?;
    Ok(text.to_owned())
}

/// Backups in `folder`, newest first. Folders that aren't complete backups are ignored.
#[cfg(test)]
pub fn list(folder: &Path) -> Vec<BackupInfo> {
    list_for(folder, &Computer::default())
}

#[cfg(test)]
pub(super) fn list_for(folder: &Path, computer: &Computer) -> Vec<BackupInfo> {
    try_list(folder, computer).unwrap_or_default()
}

/// Backups in `folder`, newest first, ignoring incomplete backups. Returns `None` if the
/// directory cannot be opened.
pub(super) fn try_list(folder: &Path, computer: &Computer) -> Option<Vec<BackupInfo>> {
    let entries = fs::read_dir(folder).ok()?;
    let mut found: Vec<BackupInfo> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let parsed = BackupName::parse(&name)?;
            let manifest = snapshot::read_manifest(&entry.path()).ok()?;
            Some(BackupInfo {
                name,
                kind: parsed.kind,
                other_computer: computer.is_other(&parsed),
                computer: parsed.tag,
                created_at: manifest.created_at.clone(),
                size_bytes: manifest.size(),
            })
        })
        .collect();
    found.sort_by(|a, b| b.name.cmp(&a.name));
    Some(found)
}

/// What a folder holds, read from the folder names alone (no manifest is opened), or `None`
/// when the folder can't be read at all. Cheap enough for [`BackupService::status`], which runs
/// often and may point at a network share or a drive that is asleep. Unlike [`try_list`] it also
/// counts a folder whose manifest is unreadable. The move works from the same names, so such a
/// backup is reported as failed (its copy can't be verified) and keeps the offer open.
pub(super) fn try_list_names(folder: &Path) -> Option<Vec<BackupName>> {
    let entries = fs::read_dir(folder).ok()?;
    Some(
        entries
            .filter_map(Result::ok)
            // `file_type` does not follow links: a link named like a backup is not one.
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| BackupName::parse(&entry.file_name().into_string().ok()?))
            .collect(),
    )
}

/// Why `new` can't receive the old folder's backups, if it can't. The new folder may not exist
/// (a removed folder or drive), so its deepest existing ancestor is looked at: if that is inside
/// the old folder, so is the new one. `symlink_metadata` does not follow links, so a link whose
/// target is missing is that ancestor too; one that can't be resolved can't be shown to be
/// outside the old folder, so it is refused (with its own message: it may lead anywhere).
pub(super) fn new_folder_problem(old: &Path, new: &Path) -> Option<&'static str> {
    let old = fs::canonicalize(old).ok()?;
    let existing = new
        .ancestors()
        .find(|ancestor| ancestor.symlink_metadata().is_ok())?;
    match fs::canonicalize(existing) {
        Ok(existing) if existing.starts_with(&old) => {
            Some("A pasta nova fica dentro da pasta anterior. Escolha outra pasta.")
        }
        Ok(_) => None,
        Err(_) => Some(
            "Não foi possível verificar a pasta nova (um atalho quebrado ou sem permissão). Escolha outra pasta.",
        ),
    }
}

/// Whether two paths are the same folder, even when one is spelled differently.
pub(super) fn same_folder(a: &Path, b: &Path) -> bool {
    a == b
        || matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
        || same_file::is_same_file(a, b).unwrap_or(false)
}

#[cfg(test)]
mod tests;
