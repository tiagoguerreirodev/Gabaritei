//! Daily automatic backups and restore (spec §3.6); the export/import data file (#24) builds on
//! the same snapshots.
//!
//! A backup is a folder `backup-<UTC time>-<computer tag>` (see [`computer_tag`]; the tag is
//! missing in names made before tags existed, or when the host name gives none) in the backup
//! folder, holding a consistent copy of the database, the images and a manifest of sha256
//! hashes (see [`snapshot`]). It is written to a hidden temporary folder and renamed, so a crash never leaves a half-written backup
//! that looks complete. The newest 7 are kept. Before a restore replaces anything, a
//! `safety-<UTC time>-<tag>` backup of the current data is made; the newest 5 of those are
//! kept. Each computer rotates only the backups with its own tag.

pub mod datafile;
pub mod relocate;
pub mod snapshot;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::ai::settings::{KEY_CLI_PATHS, KEY_REMNOTE, KEY_ROLES};
use crate::db::{Database, get_setting, set_setting};
use crate::error::{AppError, AppResult};
use crate::paths::AppPaths;

/// Setting that holds the backup folder chosen by the user.
const KEY_FOLDER: &str = "backup.folder";
/// The old folder whose backups were left behind by the last folder change (a local setting).
const KEY_LEFT_BEHIND: &str = "backup.leftBehind";
/// Settings that describe this computer. Restoring or importing data from another computer
/// must not overwrite them.
pub const LOCAL_SETTINGS: &[&str] = &[
    KEY_FOLDER,
    KEY_LEFT_BEHIND,
    KEY_CLI_PATHS,
    KEY_REMNOTE,
    KEY_ROLES,
];

/// Daily backups kept.
const KEEP_BACKUPS: usize = 7;
/// Backups made by hand with "Fazer backup agora" kept. Apart from the daily ones, so pressing
/// the button never pushes a daily backup out.
const KEEP_MANUAL: usize = 7;
/// Safety backups kept.
const KEEP_SAFETY: usize = 5;
/// A new automatic backup is due when the newest one is older than this.
const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_FOLDER: usize = 1_000;

/// Why a backup was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BackupKind {
    /// The daily automatic backup.
    Backup,
    /// "Fazer backup agora".
    Manual,
    /// Made right before a restore or import replaced the data.
    Safety,
}

impl BackupKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Backup => "backup",
            Self::Manual => "manual",
            Self::Safety => "safety",
        }
    }

    fn keep(self) -> usize {
        match self {
            Self::Backup => KEEP_BACKUPS,
            Self::Manual => KEEP_MANUAL,
            Self::Safety => KEEP_SAFETY,
        }
    }
}

/// A backup in the backup folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackupInfo {
    /// Folder name; identifies the backup in `backup_restore`.
    pub name: String,
    pub kind: BackupKind,
    /// The computer tag in the name (see [`computer_tag`]); none for backups made before tags.
    pub computer: Option<String>,
    /// Whether the backup was made by another computer (a tagged one with another tag). Such
    /// backups can be restored but are never deleted by this computer's rotation.
    pub other_computer: bool,
    pub created_at: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
}

/// The *Dados* section: where backups go, the ones that exist and the last automatic failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackupStatus {
    pub folder: String,
    pub is_default_folder: bool,
    /// Newest first.
    pub backups: Vec<BackupInfo>,
    /// pt-BR reason the last automatic or manual backup failed, until one succeeds.
    pub last_error: Option<String>,
    /// Set while the old folder still holds this computer's backups after a folder change. It is
    /// remembered across restarts until the backups are copied or moved (a copy or move with
    /// failures keeps it), the user dismisses it, the folder is changed again, or the old folder
    /// holds none of this computer's backups.
    pub left_behind: Option<LeftBehind>,
}

/// Backups that stay in the old folder after the backup folder was changed. They are not
/// moved, so they no longer show in the list, and safety backups among them can no longer be
/// used to undo an import or restore from here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LeftBehind {
    pub folder: String,
    pub backups: u32,
    /// How many of them are safety backups ("antes de restaurar").
    pub safety: u32,
}

/// What copying or moving the backups of the old folder did, and the status afterwards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RelocateOutcome {
    pub report: relocate::RelocateReport,
    /// The new folder's status. `left_behind` is what a *move* could not take out of the old
    /// folder; a copy leaves the originals on purpose and reports nothing.
    pub status: BackupStatus,
}

/// `YYYYMMDD-HHMMSS-mmm` from a timestamp like `2026-11-19T21:00:00.123Z` (see [`crate::clock`]).
fn stamp(created_at: &str) -> String {
    let digits: String = created_at.chars().filter(char::is_ascii_digit).collect();
    format!("{}-{}-{}", &digits[0..8], &digits[8..14], &digits[14..17])
}

/// Longest computer tag kept in a name.
const MAX_TAG: usize = 24;

/// This computer's tag, put at the end of the names of the backups it makes
/// (`backup-20261119-210000-123-<tag>`), so two computers sharing one synced backup folder keep
/// apart: each rotates only its own backups. Derived from the host name when the app runs (on
/// macOS the stable `LocalHostName`), reduced to lowercase letters, digits and single hyphens.
/// `None` when the host name has nothing usable; those backups are untagged, like the ones made before tags existed.
pub fn computer_tag() -> Option<String> {
    #[cfg(test)]
    {
        tests::TAG.with(|tag| tag.borrow().clone()).flatten()
    }
    #[cfg(not(test))]
    {
        static TAG: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        TAG.get_or_init(|| {
            tag_from(
                local_host_name().as_deref(),
                &gethostname::gethostname().to_string_lossy(),
            )
        })
        .clone()
    }
}

/// The tag from the host names the OS gives: the macOS `LocalHostName` when there is a usable
/// one, otherwise the network host name.
fn tag_from(local_host_name: Option<&str>, host_name: &str) -> Option<String> {
    local_host_name.and_then(slug).or_else(|| slug(host_name))
}

/// macOS keeps a stable name apart from `gethostname()`, which follows the network (a router
/// may hand out `air-3.lan` one day and `Air.local` the next). Other systems have none.
#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code))] // tests set the tag directly
fn local_host_name() -> Option<String> {
    let output = std::process::Command::new("/usr/sbin/scutil")
        .args(["--get", "LocalHostName"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(not(target_os = "macos"))]
#[cfg_attr(test, allow(dead_code))]
fn local_host_name() -> Option<String> {
    None
}

/// A plain ASCII letter for a Portuguese or other Western-European accented one.
fn unaccented(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        other => other,
    }
}

/// `Desktop_ABC 12` → `desktop-abc-12`: ASCII letters and digits (accents dropped to their
/// plain letter), other runs become one hyphen. Only the part before the first dot is used: a
/// host name often gets a network domain (`mac.local`, `mac.lan`) that changes with the
/// network, and the tag must not. A name longer than [`MAX_TAG`] is cut and ends with a short
/// hash of the whole name, so two long names that start alike keep different tags.
fn slug(text: &str) -> Option<String> {
    let name = text.split('.').next().unwrap_or_default();
    let mut out = String::new();
    // Combining marks (an accent written apart from its letter) are dropped, not turned into a
    // hyphen: `a` + U+0301 is `a`.
    let letters = name
        .chars()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .flat_map(char::to_lowercase)
        .map(unaccented);
    for c in letters {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.len() <= MAX_TAG {
        return (!out.is_empty()).then(|| out.to_owned());
    }
    // FNV-1a: stable across runs and builds, unlike the standard hasher.
    let hash = out.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    let kept = out[..MAX_TAG - 5].trim_end_matches('-');
    Some(format!("{kept}-{:04x}", hash & 0xffff))
}

/// What a backup folder name says.
#[derive(Debug, PartialEq, Eq)]
struct ParsedName {
    kind: BackupKind,
    tag: Option<String>,
}

/// Reads a backup folder name (`<kind>-<stamp>` or `<kind>-<stamp>-<tag>`), if it is one.
fn parse(name: &str) -> Option<ParsedName> {
    let (kind, rest) = [BackupKind::Backup, BackupKind::Manual, BackupKind::Safety]
        .into_iter()
        .find_map(|kind| Some((kind, name.strip_prefix(kind.prefix())?.strip_prefix('-')?)))?;
    let (stamp, tag) = match rest.split_at_checked(19)? {
        (stamp, "") => (stamp, None),
        (stamp, tag) => (stamp, Some(tag.strip_prefix('-')?)),
    };
    let shape = stamp.bytes().enumerate().all(|(index, byte)| match index {
        8 | 15 => byte == b'-',
        _ => byte.is_ascii_digit(),
    });
    let tag_ok = tag.is_none_or(|tag| {
        !tag.is_empty()
            && !tag.starts_with('-')
            && !tag.ends_with('-')
            && !tag.contains("--")
            && tag.len() <= MAX_TAG
            && tag
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    });
    (shape && tag_ok).then(|| ParsedName {
        kind,
        tag: tag.map(str::to_owned),
    })
}

/// The kind a folder name says it is, if it is a backup name at all.
fn parse_name(name: &str) -> Option<BackupKind> {
    parse(name).map(|parsed| parsed.kind)
}

/// Whether a backup tagged `tag` was made by another computer than the one running.
fn is_other_computer(tag: Option<&str>) -> bool {
    tag.is_some() && tag != computer_tag().as_deref()
}

/// Whether a backup in a shared folder is not this computer's to copy or delete: another
/// computer's, or (on a computer that has a tag) an untagged one, which can't be told apart.
/// Rotation leaves these alone for the same reason.
fn leave_alone(info: &BackupInfo) -> bool {
    info.other_computer || (info.computer.is_none() && computer_tag().is_some())
}

/// The default backup folder.
pub fn default_folder(paths: &AppPaths) -> PathBuf {
    paths.data_dir().join("backups")
}

/// A folder the user can open from Configurações → Dados. The frontend only picks one of
/// these; it never sends a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum OpenableFolder {
    /// The app data folder (database, images).
    Data,
    /// The backup folder.
    Backups,
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
    Ok(get_setting::<String>(conn, KEY_FOLDER)?
        .map(PathBuf::from)
        .unwrap_or_else(|| default_folder(paths)))
}

/// Saves the backup folder (`None` goes back to the default) after checking it can be written.
pub fn set_folder(conn: &Connection, paths: &AppPaths, chosen: Option<&Path>) -> AppResult<()> {
    let Some(chosen) = chosen else {
        crate::db::put_raw_setting(conn, KEY_FOLDER, None)?;
        return Ok(());
    };
    let text = chosen
        .to_str()
        .filter(|text| !text.trim().is_empty() && text.chars().count() <= MAX_FOLDER)
        .ok_or_else(|| AppError::Invalid("Escolha uma pasta válida para os backups.".into()))?;
    if !chosen.is_absolute() {
        return Err(AppError::Invalid(
            "Escolha uma pasta válida para os backups.".into(),
        ));
    }
    if chosen.starts_with(paths.images_dir()) {
        return Err(AppError::Invalid(
            "Escolha uma pasta fora da pasta de imagens do Gabaritei.".into(),
        ));
    }
    // Inside a backup (or its hidden build folder) rotation could later delete the chosen folder.
    let inside_a_backup = |path: &Path| {
        path.components().any(|part| {
            part.as_os_str()
                .to_str()
                .is_some_and(|name| name.starts_with(".tmp-") || parse(name).is_some())
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
    // inside the backup.
    let leads_into_a_backup =
        |path: &Path| fs::canonicalize(path).is_ok_and(|real| inside_a_backup(&real));
    if chosen
        .ancestors()
        .find(|ancestor| ancestor.exists())
        .is_some_and(leads_into_a_backup)
    {
        return Err(refuse());
    }
    fs::create_dir_all(chosen).map_err(|_| not_writable())?;
    // Checked again once it exists, in case the path changed meanwhile.
    if leads_into_a_backup(chosen) {
        return Err(refuse());
    }
    tempfile::tempfile_in(chosen).map_err(|_| not_writable())?;
    set_setting(conn, KEY_FOLDER, &text)
}

/// Backups in `folder`, newest first. Folders that aren't complete backups are ignored.
pub fn list(folder: &Path) -> Vec<BackupInfo> {
    try_list(folder).unwrap_or_default()
}

/// Like [`list`], but `None` when the folder can't be read at all (a drive that is unplugged, a
/// share that is offline), which is not the same as a folder with no backups.
fn try_list(folder: &Path) -> Option<Vec<BackupInfo>> {
    let entries = fs::read_dir(folder).ok()?;
    let mut found: Vec<BackupInfo> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let parsed = parse(&name)?;
            let manifest = snapshot::read_manifest(&entry.path()).ok()?;
            Some(BackupInfo {
                name,
                kind: parsed.kind,
                other_computer: is_other_computer(parsed.tag.as_deref()),
                computer: parsed.tag,
                created_at: manifest.created_at.clone(),
                size_bytes: manifest.size(),
            })
        })
        .collect();
    found.sort_by(|a, b| b.name.cmp(&a.name));
    Some(found)
}

/// The notice for backups `left` in `old`, if there are any.
fn left_behind(old: &Path, left: &[BackupInfo]) -> Option<LeftBehind> {
    (!left.is_empty()).then(|| LeftBehind {
        folder: old.display().to_string(),
        backups: left.len() as u32,
        safety: left
            .iter()
            .filter(|info| info.kind == BackupKind::Safety)
            .count() as u32,
    })
}

/// Stores (or, with `None`, forgets) the old folder whose backups were left behind.
fn remember_old_folder(conn: &Connection, old: Option<&Path>) -> AppResult<()> {
    match old {
        Some(old) => set_setting(conn, KEY_LEFT_BEHIND, &old.display().to_string()),
        None => crate::db::put_raw_setting(conn, KEY_LEFT_BEHIND, None),
    }
}

/// The remembered old folder and what it still holds of this computer's, when there is
/// something to offer. A stale memory (the folder is the current one now, or has nothing of ours
/// left) is forgotten here.
fn pending_old_folder(db: &Database, current: &Path) -> AppResult<Option<(PathBuf, LeftBehind)>> {
    let Some(old) = db.with(|conn| get_setting::<String>(conn, KEY_LEFT_BEHIND))? else {
        return Ok(None);
    };
    let old = PathBuf::from(old);
    let mine: Vec<BackupInfo> = if same_folder(&old, current) {
        Vec::new()
    } else {
        // An unreadable folder hides the offer for now but keeps it: the drive may come back.
        let Some(found) = try_list(&old) else {
            return Ok(None);
        };
        found
            .into_iter()
            .filter(|info| !leave_alone(info))
            .collect()
    };
    let notice = left_behind(&old, &mine);
    if notice.is_none() {
        forget_old_folder_if(db, &old)?;
    }
    Ok(notice.map(|notice| (old, notice)))
}

/// Forgets the remembered old folder only if it is still `old`. `status` runs without the backup
/// lock, so a folder change could have stored a newer one since it was read; that one stays.
fn forget_old_folder_if(db: &Database, old: &Path) -> AppResult<()> {
    db.with(|conn| {
        let stored = get_setting::<String>(conn, KEY_LEFT_BEHIND)?;
        if stored.as_deref() == Some(old.display().to_string().as_str()) {
            remember_old_folder(conn, None)?;
        }
        Ok(())
    })
}

/// Whether two paths are the same folder, even when one is spelled differently.
fn same_folder(a: &Path, b: &Path) -> bool {
    a == b
        || matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
        || same_file::is_same_file(a, b).unwrap_or(false)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs backup, restore and import one at a time, and remembers why the last backup failed.
/// Managed as Tauri state.
#[derive(Default)]
pub struct BackupService {
    busy: Mutex<()>,
    last_error: Mutex<Option<String>>,
}

impl BackupService {
    /// Runs `work` once nothing else is backing up, restoring or importing.
    pub fn exclusive<T>(&self, work: impl FnOnce() -> T) -> T {
        let _turn = lock(&self.busy);
        work()
    }

    fn remember<T>(&self, result: &AppResult<T>) {
        *lock(&self.last_error) = result.as_ref().err().map(ToString::to_string);
    }

    /// Whether a backup, restore or import is running now.
    pub fn is_busy(&self) -> bool {
        self.busy
            .try_lock()
            .is_err_and(|error| matches!(error, std::sync::TryLockError::WouldBlock))
    }

    /// The *Dados* section's data.
    pub fn status(&self, db: &Database, paths: &AppPaths) -> AppResult<BackupStatus> {
        let (folder, is_default_folder) = db.with(|conn| {
            let chosen = get_setting::<String>(conn, KEY_FOLDER)?;
            Ok((folder(conn, paths)?, chosen.is_none()))
        })?;
        let left_behind = pending_old_folder(db, &folder)?.map(|(_, notice)| notice);
        Ok(BackupStatus {
            backups: list(&folder),
            folder: folder.display().to_string(),
            is_default_folder,
            last_error: lock(&self.last_error).clone(),
            left_behind,
        })
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
        // Not while a backup is being written: it could land in the old folder after the
        // count below.
        self.exclusive(|| {
            let old = db.with(|conn| folder(conn, paths))?;
            db.with(|conn| set_folder(conn, paths, chosen))?;
            let new = db.with(|conn| folder(conn, paths))?;
            let has_backups =
                !same_folder(&old, &new) && list(&old).iter().any(|i| !leave_alone(i));
            db.with(|conn| remember_old_folder(conn, has_backups.then_some(old.as_path())))?;
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
    fn relocate_with(
        &self,
        db: &Database,
        paths: &AppPaths,
        remove_originals: bool,
        between: &dyn Fn(),
    ) -> AppResult<RelocateOutcome> {
        // The lock is taken per backup, not for the whole copy: a scheduled backup can run between
        // two of them, and the folders are read again each time.
        let (old, new, names, others) = self.exclusive(|| {
            let nothing = || AppError::Invalid("Não há backups na pasta anterior.".into());
            let new = db.with(|conn| folder(conn, paths))?;
            // Read from the stored setting, so it works after a restart too, and never from a
            // path the frontend sent. It is cleared when the old folder has nothing of ours left.
            let (old, _) = pending_old_folder(db, &new)?.ok_or_else(nothing)?;
            // A new folder inside the old one (so inside a backup, or a temporary folder) would
            // be deleted along with the original on a move.
            let nested = match (fs::canonicalize(&old), fs::canonicalize(&new)) {
                (Ok(old), Ok(new)) => new.starts_with(&old),
                _ => false,
            };
            if nested {
                return Err(AppError::Invalid(
                    "A pasta nova fica dentro da pasta anterior. Escolha outra pasta.".into(),
                ));
            }
            // Another computer's backups, and untagged ones on a tagged computer, stay where they are:
            // they may belong to a computer that still uses that folder.
            let (others, mine): (Vec<_>, Vec<_>) = list(&old).into_iter().partition(leave_alone);
            let names: Vec<String> = mine.into_iter().map(|info| info.name).collect();
            Ok((old, new, names, others.len() as u32))
        })?;
        let still_the_same = || {
            let current = db.with(|conn| {
                Ok((
                    folder(conn, paths)?,
                    get_setting::<String>(conn, KEY_LEFT_BEHIND)?,
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
                let remaining = (remove_originals || report.failed > 0)
                    && list(&old).iter().any(|info| !leave_alone(info));
                db.with(|conn| remember_old_folder(conn, remaining.then_some(old.as_path())))?;
            }
            let status = self.status(db, paths)?;
            Ok(RelocateOutcome { report, status })
        })
    }

    /// Makes a backup now.
    pub fn backup_now(&self, db: &Database, paths: &AppPaths, now: &str) -> AppResult<BackupInfo> {
        self.exclusive(|| {
            let result = db
                .with(|conn| folder(conn, paths))
                .and_then(|folder| create(db, paths, &folder, BackupKind::Manual, now, true));
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
                if is_due(&folder, now) {
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
            restore(db, paths, name, now)
        })
    }
}

/// Whether the newest backup of this computer is older than a day (or there is none).
/// A backup another computer made into a shared folder doesn't count.
fn is_due(folder: &Path, now: OffsetDateTime) -> bool {
    let cutoff = crate::clock::format(now - INTERVAL);
    let present = crate::clock::format(now);
    list(folder)
        .iter()
        // A backup dated in the future (the clock was wrong once) must not stop new ones.
        .find(|info| {
            info.kind == BackupKind::Backup && !info.other_computer && info.created_at <= present
        })
        .is_none_or(|newest| newest.created_at < cutoff)
}

/// Writes one backup into `folder`: into a hidden temporary folder first, then renamed.
fn create(
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
fn create_with(
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
struct Pending {
    name: String,
    computer: Option<String>,
    building: PathBuf,
    finished: PathBuf,
}

/// Creates the empty temporary folder for a backup of `kind` made at `now`.
fn begin(folder: &Path, kind: BackupKind, now: &str) -> AppResult<Pending> {
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
    fn abandon(&self) {
        remove_dir_logged(&self.building);
    }

    /// Renames the temporary folder to its final name (the backup now exists) and, if asked,
    /// prunes the old backups of its kind.
    fn finish(
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
fn is_stale(path: &Path) -> bool {
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
fn prune(folder: &Path, kind: BackupKind) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    let own = computer_tag();
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
                .is_some_and(|parsed| parsed.tag.is_some() && parsed.tag != own);
            let mine = parsed.is_some_and(|parsed| parsed.tag == own);
            if mine || (!theirs && is_stale(&entry.path())) {
                remove_dir_logged(&entry.path());
            }
        } else if parse(&name).is_some_and(|parsed| parsed.kind == kind && parsed.tag == own) {
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

/// Replaces the current data with the backup called `name`:
///
/// 1. checks the backup completely (hashes, `integrity_check`, schema version): a bad backup
///    stops here and nothing changes, not even a safety backup;
/// 2. makes a safety backup of the current data;
/// 3. replaces the database and adds the images.
///
/// This computer's own settings (backup folder, CLI paths) are kept.
pub fn restore(db: &Database, paths: &AppPaths, name: &str, now: &str) -> AppResult<()> {
    if parse_name(name).is_none() {
        return Err(AppError::Invalid("Backup não encontrado.".into()));
    }
    let folder = db.with(|conn| folder(conn, paths))?;
    let source = folder.join(name);
    if !source.is_dir() {
        return Err(AppError::Invalid("Backup não encontrado.".into()));
    }
    replace_data(db, paths, &folder, &source, now)
}

/// Replaces the data with the snapshot in `source`: checks and stages it (a bad snapshot stops
/// here and nothing changes, not even a safety backup), makes a safety backup of the current
/// data, then replaces it. Shared with the import of a data file.
pub fn replace_data(
    db: &Database,
    paths: &AppPaths,
    folder: &Path,
    source: &Path,
    now: &str,
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
        snapshot::commit(conn, &staged, LOCAL_SETTINGS)
    })?;
    prune(folder, BackupKind::Safety);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use time::macros::datetime;

    thread_local! {
        /// Overrides [`computer_tag`] for the current test: `Some(None)` is "no tag".
        /// Without an override, tests make untagged backups.
        pub(super) static TAG: std::cell::RefCell<Option<Option<String>>> =
            const { std::cell::RefCell::new(None) };
    }

    fn as_computer(tag: &str) {
        TAG.with(|current| *current.borrow_mut() = Some(Some(tag.to_owned())));
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        paths: AppPaths,
        db: Database,
    }

    const IMAGE: &str = "0123456789abcdef01234567.png";

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().join("data"));
        let db = Database::open(&paths.database()).unwrap();
        fs::create_dir_all(paths.images_dir()).unwrap();
        fs::write(paths.images_dir().join(IMAGE), b"png bytes").unwrap();
        Fixture {
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

    fn question_ids(db: &Database) -> Vec<String> {
        db.with(|conn| {
            let mut stmt = conn.prepare("SELECT id FROM questions ORDER BY id")?;
            Ok(stmt
                .query_map([], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()?)
        })
        .unwrap()
    }

    fn backup(f: &Fixture, now: &str) -> BackupInfo {
        let folder = default_folder(&f.paths);
        create(&f.db, &f.paths, &folder, BackupKind::Backup, now, true).unwrap()
    }

    #[test]
    fn a_consistent_backup_restores_the_answers_and_passes_the_integrity_check() {
        let f = fixture();
        add_question(&f.db, "q-1");
        add_question(&f.db, "q-2");
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        assert_eq!(info.name, "backup-20261119-100000-000");

        // The data changes after the backup: one question is lost, another one added.
        f.db.with(|conn| {
            conn.execute("DELETE FROM questions WHERE id = 'q-2'", [])?;
            Ok(())
        })
        .unwrap();
        add_question(&f.db, "q-3");

        restore(&f.db, &f.paths, &info.name, "2026-11-19T11:00:00.000Z").unwrap();

        assert_eq!(question_ids(&f.db), ["q-1", "q-2"]);
        f.db.with(|conn| {
            let integrity: String =
                conn.pragma_query_value(None, "integrity_check", |row| row.get(0))?;
            assert_eq!(integrity, "ok");
            let mode: String = conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
            assert_eq!(mode, "wal");
            Ok(())
        })
        .unwrap();
        // The database keeps working after the replacement.
        add_question(&f.db, "q-4");
    }

    /// Rewrites the backup's database with another page size and fixes the manifest hash, like a
    /// file made by a build that uses a different page size.
    fn change_page_size(folder: &Path, name: &str) {
        let dir = folder.join(name);
        let database = dir.join(snapshot::DATABASE_FILE);
        let conn = Connection::open(&database).unwrap();
        conn.pragma_update(None, "page_size", 8192).unwrap();
        conn.execute_batch("VACUUM").unwrap();
        conn.close().unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(snapshot::MANIFEST_FILE)).unwrap()).unwrap();
        let (sha256, size) = snapshot::hash_file(&database).unwrap();
        for file in manifest["files"].as_array_mut().unwrap() {
            if file["path"] == snapshot::DATABASE_FILE {
                file["sha256"] = sha256.clone().into();
                file["size"] = size.into();
            }
        }
        fs::write(
            dir.join(snapshot::MANIFEST_FILE),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn a_page_size_mismatch_is_refused_before_the_safety_backup_so_nothing_changes() {
        let f = fixture();
        add_question(&f.db, "q-1");
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        let folder = default_folder(&f.paths);
        change_page_size(&folder, &info.name);
        add_question(&f.db, "q-2");

        let error = restore(&f.db, &f.paths, &info.name, "2026-11-19T11:00:00.000Z").unwrap_err();

        assert!(matches!(error, AppError::Invalid(_)), "{error:?}");
        assert!(error.to_string().contains("Nada foi alterado"));
        assert_eq!(question_ids(&f.db), ["q-1", "q-2"]);
        assert!(
            list(&folder)
                .iter()
                .all(|info| info.kind != BackupKind::Safety),
            "no safety backup for a replacement that was refused"
        );
    }

    #[test]
    fn the_folders_to_open_are_the_data_folder_and_the_current_backup_folder() {
        let f = fixture();

        let data = folder_to_open(&f.db, &f.paths, OpenableFolder::Data).unwrap();
        assert_eq!(data, f.paths.data_dir());

        // The backup folder doesn't exist before the first backup: it is created, not an error.
        let default = default_folder(&f.paths);
        assert!(!default.exists());
        let backups = folder_to_open(&f.db, &f.paths, OpenableFolder::Backups).unwrap();
        assert_eq!(backups, default);
        assert!(default.is_dir());

        // After the user picks another folder, that is the one to open.
        let chosen = f._dir.path().join("elsewhere");
        f.db.with(|conn| set_folder(conn, &f.paths, Some(&chosen)))
            .unwrap();
        let backups = folder_to_open(&f.db, &f.paths, OpenableFolder::Backups).unwrap();
        assert_eq!(backups, chosen);
    }

    #[test]
    fn the_openable_folder_is_a_closed_choice_not_a_path() {
        assert_eq!(
            serde_json::from_str::<OpenableFolder>("\"backups\"").unwrap(),
            OpenableFolder::Backups
        );
        assert!(serde_json::from_str::<OpenableFolder>("\"/etc\"").is_err());
    }

    #[test]
    fn changing_the_folder_reports_the_backups_left_in_the_old_one() {
        let f = fixture();
        let service = BackupService::default();
        let old = default_folder(&f.paths);
        create(
            &f.db,
            &f.paths,
            &old,
            BackupKind::Backup,
            "2026-11-19T10:00:00.000Z",
            true,
        )
        .unwrap();
        create(
            &f.db,
            &f.paths,
            &old,
            BackupKind::Safety,
            "2026-11-19T11:00:00.000Z",
            false,
        )
        .unwrap();
        let elsewhere = f._dir.path().join("synced");

        let status = service
            .change_folder(&f.db, &f.paths, Some(&elsewhere))
            .unwrap();

        assert_eq!(status.folder, elsewhere.display().to_string());
        assert!(status.backups.is_empty());
        assert_eq!(
            status.left_behind,
            Some(LeftBehind {
                folder: old.display().to_string(),
                backups: 2,
                safety: 1,
            })
        );
        // Nothing was moved or deleted.
        assert_eq!(list(&old).len(), 2);
        // The plain status carries it too (it is remembered, see #111).
        assert_eq!(
            service.status(&f.db, &f.paths).unwrap().left_behind,
            status.left_behind
        );
        // Choosing the same folder again, or an empty old folder, has nothing left behind.
        let again = service
            .change_folder(&f.db, &f.paths, Some(&elsewhere))
            .unwrap();
        assert_eq!(again.left_behind, None);
        let back = service.change_folder(&f.db, &f.paths, None).unwrap();
        assert_eq!(back.left_behind, None);
        assert!(back.is_default_folder);
    }

    #[test]
    fn restoring_makes_a_safety_backup_of_what_it_replaces() {
        let f = fixture();
        add_question(&f.db, "q-1");
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        add_question(&f.db, "q-2");

        restore(&f.db, &f.paths, &info.name, "2026-11-19T11:00:00.000Z").unwrap();

        let all = list(&default_folder(&f.paths));
        let safety = all
            .iter()
            .find(|info| info.kind == BackupKind::Safety)
            .expect("a safety backup exists");
        assert_eq!(safety.name, "safety-20261119-110000-000");
        // Restoring the safety backup brings the replaced data back.
        restore(&f.db, &f.paths, &safety.name, "2026-11-19T12:00:00.000Z").unwrap();
        assert_eq!(question_ids(&f.db), ["q-1", "q-2"]);
    }

    #[test]
    fn a_corrupt_backup_changes_nothing() {
        let f = fixture();
        add_question(&f.db, "q-1");
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        add_question(&f.db, "q-2");
        let folder = default_folder(&f.paths);
        let dir = folder.join(&info.name);

        // A flipped byte in the database fails the hash.
        let database = dir.join(snapshot::DATABASE_FILE);
        let mut bytes = fs::read(&database).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        fs::write(&database, bytes).unwrap();

        let error = restore(&f.db, &f.paths, &info.name, "2026-11-19T11:00:00.000Z").unwrap_err();
        assert!(error.to_string().contains("corrompidos"), "{error}");
        assert_eq!(question_ids(&f.db), ["q-1", "q-2"]);
        assert!(
            list(&folder).iter().all(|b| b.kind == BackupKind::Backup),
            "no safety backup for a backup that was rejected"
        );
    }

    #[test]
    fn a_backup_with_a_bad_database_or_missing_files_is_rejected() {
        let f = fixture();
        add_question(&f.db, "q-1");
        let folder = default_folder(&f.paths);

        // Missing image.
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        fs::remove_file(folder.join(&info.name).join("images").join(IMAGE)).unwrap();
        assert!(snapshot::verify(&folder.join(&info.name)).is_err());

        // A database that is not SQLite, with a matching hash.
        let info = backup(&f, "2026-11-19T10:01:00.000Z");
        let dir = folder.join(&info.name);
        let database = dir.join(snapshot::DATABASE_FILE);
        fs::write(&database, b"definitely not a database").unwrap();
        let mut manifest = snapshot::read_manifest(&dir).unwrap();
        let (sha256, size) = snapshot::hash_file(&database).unwrap();
        manifest.files[0].sha256 = sha256;
        manifest.files[0].size = size;
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(snapshot::verify(&dir).is_err());

        // A manifest that points outside the snapshot.
        let info = backup(&f, "2026-11-19T10:02:00.000Z");
        let dir = folder.join(&info.name);
        let mut manifest = snapshot::read_manifest(&dir).unwrap();
        manifest.files[0].path = "../gabaritei.sqlite".into();
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(snapshot::verify(&dir).is_err());

        assert_eq!(question_ids(&f.db), ["q-1"]);
    }

    /// Runs `sql` on a backup's database and fixes the manifest hash, as someone building a
    /// hostile file would.
    fn doctor(f: &Fixture, info: &BackupInfo, sql: &str) {
        let dir = default_folder(&f.paths).join(&info.name);
        let database = dir.join(snapshot::DATABASE_FILE);
        Connection::open(&database)
            .unwrap()
            .execute_batch(sql)
            .unwrap();
        let mut manifest = snapshot::read_manifest(&dir).unwrap();
        let (sha256, size) = snapshot::hash_file(&database).unwrap();
        manifest.files[0].sha256 = sha256;
        manifest.files[0].size = size;
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn a_database_with_another_schema_is_rejected_before_anything_changes() {
        for sql in [
            "DROP TABLE settings",
            "CREATE TRIGGER evil AFTER INSERT ON responses BEGIN DELETE FROM settings; END",
            "CREATE VIEW sneaky AS SELECT 1",
            // A name that a `LIKE 'sqlite_%'` filter would have taken for an internal one.
            "CREATE TRIGGER sqliteXevil AFTER INSERT ON responses BEGIN DELETE FROM settings; END",
            "CREATE TABLE sqliteXextra (x TEXT) STRICT",
            "CREATE TABLE extra (x TEXT) STRICT",
            // A row whose `type` says index but whose SQL is a trigger, as a hand-built file could.
            "PRAGMA writable_schema = ON; INSERT INTO sqlite_schema VALUES ('index', 'sqlite_evil', 'responses', 0, 'CREATE TRIGGER sqlite_evil AFTER INSERT ON responses BEGIN DELETE FROM settings; END')",
        ] {
            let f = fixture();
            add_question(&f.db, "q-1");
            let info = backup(&f, "2026-11-19T10:00:00.000Z");
            doctor(&f, &info, sql);
            add_question(&f.db, "q-2");
            let folder = default_folder(&f.paths);

            let error =
                restore(&f.db, &f.paths, &info.name, "2026-11-19T11:00:00.000Z").unwrap_err();

            assert!(
                error.to_string().contains("Nada foi alterado"),
                "{sql}: {error}"
            );
            assert_eq!(question_ids(&f.db), ["q-1", "q-2"], "{sql}");
            assert!(
                list(&folder).iter().all(|b| b.kind == BackupKind::Backup),
                "{sql}: no safety backup"
            );
            // The app still works: its settings and its backup folder are intact.
            f.db.with(|conn| folder_of(conn, &f.paths)).unwrap();
        }
    }

    fn folder_of(conn: &Connection, paths: &AppPaths) -> AppResult<PathBuf> {
        folder(conn, paths)
    }

    #[test]
    fn a_backup_from_a_newer_schema_is_rejected() {
        let f = fixture();
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        let dir = default_folder(&f.paths).join(&info.name);
        let mut manifest = snapshot::read_manifest(&dir).unwrap();
        manifest.schema_version = crate::db::SCHEMA_VERSION + 1;
        fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let error = snapshot::verify(&dir).unwrap_err();
        assert_eq!(error.to_string(), crate::db::NEWER_SCHEMA_MESSAGE);
    }

    #[test]
    fn restore_only_accepts_backup_names() {
        let f = fixture();
        for name in [
            "../x",
            "backup-1",
            "other",
            "backup-20261119-100000-00x",
            "",
        ] {
            assert!(restore(&f.db, &f.paths, name, "2026-11-19T11:00:00.000Z").is_err());
        }
    }

    #[test]
    fn images_copied_ahead_of_the_lock_are_kept_and_later_ones_are_added() {
        let f = fixture();
        let late = "fedcba9876543210fedcba98.png";
        let dest = f.paths.data_dir().join("snapshot");
        fs::create_dir_all(&dest).unwrap();
        // Early copy: only IMAGE exists yet.
        let ahead = snapshot::copy_images_ahead(&f.paths.images_dir(), &dest).unwrap();
        assert_eq!(ahead.len(), 1);
        // An image added between the early copy and the database copy.
        fs::write(f.paths.images_dir().join(late), b"late bytes").unwrap();
        let manifest =
            f.db.with(|conn| {
                snapshot::write_locked(
                    conn,
                    &f.paths.images_dir(),
                    &dest,
                    "2026-11-19T10:00:00.000Z",
                    ahead,
                )
            })
            .unwrap();
        let paths: Vec<&str> = manifest
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect();
        assert_eq!(
            paths,
            [
                "gabaritei.sqlite",
                &format!("images/{IMAGE}"),
                &format!("images/{late}")
            ]
        );
        // The snapshot is complete and every hash matches.
        snapshot::verify(&dest).unwrap();
    }

    #[test]
    fn a_restore_still_puts_every_image_in_its_safety_backup() {
        let f = fixture();
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        let extra = "fedcba9876543210fedcba98.png";
        fs::write(f.paths.images_dir().join(extra), b"extra").unwrap();
        restore(&f.db, &f.paths, &info.name, "2026-11-19T11:00:00.000Z").unwrap();
        let safety = default_folder(&f.paths).join("safety-20261119-110000-000");
        assert_eq!(
            fs::read(safety.join("images").join(extra)).unwrap(),
            b"extra"
        );
        assert_eq!(
            fs::read(safety.join("images").join(IMAGE)).unwrap(),
            b"png bytes"
        );
        snapshot::verify(&safety).unwrap();
        assert!(
            !default_folder(&f.paths).read_dir().unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp-")),
            "no temporary folder left"
        );
    }

    #[test]
    fn images_are_backed_up_and_come_back() {
        let f = fixture();
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        let dir = default_folder(&f.paths).join(&info.name);
        assert_eq!(
            fs::read(dir.join("images").join(IMAGE)).unwrap(),
            b"png bytes"
        );
        // Not an image name: never copied.
        fs::write(f.paths.images_dir().join("notes.txt"), b"x").unwrap();
        let second = backup(&f, "2026-11-19T10:01:00.000Z");
        assert!(
            !default_folder(&f.paths)
                .join(&second.name)
                .join("images/notes.txt")
                .exists()
        );

        fs::remove_file(f.paths.images_dir().join(IMAGE)).unwrap();
        restore(&f.db, &f.paths, &info.name, "2026-11-19T11:00:00.000Z").unwrap();
        assert_eq!(
            fs::read(f.paths.images_dir().join(IMAGE)).unwrap(),
            b"png bytes"
        );
    }

    #[test]
    fn restoring_keeps_this_computers_own_settings() {
        let f = fixture();
        f.db.with(|conn| {
            set_setting(conn, KEY_CLI_PATHS, &"windows-path")?;
            set_setting(conn, KEY_REMNOTE, &"http://remote.example/mcp")?;
            set_setting(conn, KEY_ROLES, &"roles from the other computer")?;
            set_setting(conn, "other", &"from the backup")
        })
        .unwrap();
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        f.db.with(|conn| {
            set_setting(conn, KEY_CLI_PATHS, &"mac-path")?;
            set_setting(conn, KEY_REMNOTE, &"http://localhost/mcp")?;
            set_setting(conn, KEY_ROLES, &"roles of this computer")?;
            set_setting(conn, "other", &"changed")?;
            set_setting(conn, KEY_FOLDER, &"/elsewhere")
        })
        .unwrap();

        // The folder setting points at /elsewhere now, so the backup is found by moving it.
        let elsewhere = tempfile::tempdir().unwrap();
        f.db.with(|conn| set_setting(conn, KEY_FOLDER, &elsewhere.path().to_str().unwrap()))
            .unwrap();
        fs::rename(
            default_folder(&f.paths).join(&info.name),
            elsewhere.path().join(&info.name),
        )
        .unwrap();
        restore(&f.db, &f.paths, &info.name, "2026-11-19T11:00:00.000Z").unwrap();

        f.db.with(|conn| {
            assert_eq!(
                get_setting::<String>(conn, "other")?.as_deref(),
                Some("from the backup")
            );
            assert_eq!(
                get_setting::<String>(conn, KEY_CLI_PATHS)?.as_deref(),
                Some("mac-path")
            );
            assert_eq!(
                get_setting::<String>(conn, KEY_REMNOTE)?.as_deref(),
                Some("http://localhost/mcp")
            );
            assert_eq!(
                get_setting::<String>(conn, KEY_ROLES)?.as_deref(),
                Some("roles of this computer")
            );
            assert_eq!(
                get_setting::<String>(conn, KEY_FOLDER)?.as_deref(),
                elsewhere.path().to_str()
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn restoring_rebuilds_reviews_from_the_restored_answers() {
        let f = fixture();
        add_question(&f.db, "q-1");
        // A review the answers don't justify, as an older build could have left in a backup.
        f.db.with(|conn| {
            conn.execute(
                "INSERT INTO review_state (question_id, due_at, state)
                 VALUES ('q-1', '2026-11-20T00:00:00Z', '{}')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        restore(&f.db, &f.paths, &info.name, "2026-11-19T11:00:00.000Z").unwrap();
        let reviews: i64 = f
            .db
            .with(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM review_state", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(reviews, 0);
    }

    #[test]
    fn keeps_the_newest_seven_backups_and_five_safety_backups() {
        let f = fixture();
        for day in 1..=9 {
            backup(&f, &format!("2026-11-{day:02}T10:00:00.000Z"));
        }
        let folder = default_folder(&f.paths);
        let names: Vec<String> = list(&folder).into_iter().map(|b| b.name).collect();
        assert_eq!(names.len(), 7);
        assert_eq!(names[0], "backup-20261109-100000-000");
        assert_eq!(names[6], "backup-20261103-100000-000");

        for minute in 0..7 {
            restore(
                &f.db,
                &f.paths,
                "backup-20261109-100000-000",
                &format!("2026-11-10T10:{minute:02}:00.000Z"),
            )
            .unwrap();
        }
        let all = list(&folder);
        assert_eq!(
            all.iter().filter(|b| b.kind == BackupKind::Safety).count(),
            5
        );
        assert_eq!(
            all.iter().filter(|b| b.kind == BackupKind::Backup).count(),
            7
        );
    }

    #[test]
    fn manual_backups_are_kept_apart_from_the_daily_ones() {
        let f = fixture();
        let folder = default_folder(&f.paths);
        let service = BackupService::default();
        for day in 1..=3 {
            backup(&f, &format!("2026-11-{day:02}T10:00:00.000Z"));
        }
        // Pressing the button many times never pushes a daily backup out...
        for minute in 0..9 {
            let made = service
                .backup_now(
                    &f.db,
                    &f.paths,
                    &format!("2026-11-04T10:{minute:02}:00.000Z"),
                )
                .unwrap();
            assert_eq!(made.kind, BackupKind::Manual);
            assert!(made.name.starts_with("manual-"));
        }
        let all = list(&folder);
        assert_eq!(
            all.iter().filter(|b| b.kind == BackupKind::Backup).count(),
            3
        );
        // ...and keeps only the newest manual ones.
        assert_eq!(
            all.iter().filter(|b| b.kind == BackupKind::Manual).count(),
            7
        );
        assert!(all.iter().all(|b| b.name != "manual-20261104-100000-000"));
        // Manual backups are not the daily one: it is due a day after the last daily backup,
        // however many manual ones exist.
        let at = |time| service.backup_if_due(&f.db, &f.paths, time).unwrap();
        assert!(at(datetime!(2026-11-04 09:30:00 UTC)).is_none());
        assert!(at(datetime!(2026-11-04 11:00:00 UTC)).is_some());
        // A manual backup can be restored like any other.
        restore(
            &f.db,
            &f.paths,
            "manual-20261104-100800-000",
            "2026-11-06T10:00:00.000Z",
        )
        .unwrap();
    }

    #[test]
    fn a_backup_is_due_after_a_day_and_ignores_safety_backups_and_leftovers() {
        let f = fixture();
        let folder = default_folder(&f.paths);
        let service = BackupService::default();
        let at = |text: OffsetDateTime| service.backup_if_due(&f.db, &f.paths, text).unwrap();

        // None yet: one is made. Right after, nothing is due.
        assert!(at(datetime!(2026-11-19 10:00:00 UTC)).is_some());
        assert!(at(datetime!(2026-11-19 11:00:00 UTC)).is_none());
        assert!(at(datetime!(2026-11-20 09:59:59 UTC)).is_none());
        assert!(at(datetime!(2026-11-20 10:00:01 UTC)).is_some());
        assert_eq!(list(&folder).len(), 2);

        // A leftover temporary folder is not a backup, and is cleaned at the next backup.
        fs::create_dir_all(folder.join(".tmp-backup-20261201-000000-000")).unwrap();
        assert_eq!(list(&folder).len(), 2);
        assert!(at(datetime!(2026-11-22 10:00:00 UTC)).is_some());
        assert!(!folder.join(".tmp-backup-20261201-000000-000").exists());
    }

    #[test]
    fn a_backup_dated_in_the_future_does_not_stop_new_ones() {
        let f = fixture();
        let service = BackupService::default();
        // The clock was wrong once and made a backup dated far ahead.
        backup(&f, "2030-01-01T10:00:00.000Z");
        let made = service
            .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 10:00:00 UTC))
            .unwrap();
        assert!(made.is_some());
    }

    #[test]
    fn a_failed_backup_leaves_no_partial_folder_and_is_reported() {
        let f = fixture();
        let service = BackupService::default();
        // The backup folder is a file: nothing can be created in it.
        let blocker = f.paths.data_dir().join("blocked");
        fs::write(&blocker, b"file").unwrap();
        f.db.with(|conn| set_setting(conn, KEY_FOLDER, &blocker.to_str().unwrap()))
            .unwrap();

        assert!(
            service
                .backup_now(&f.db, &f.paths, "2026-11-19T10:00:00.000Z")
                .is_err()
        );
        let status = service.status(&f.db, &f.paths).unwrap();
        assert!(status.last_error.is_some());

        f.db.with(|conn| {
            set_setting(
                conn,
                KEY_FOLDER,
                &f.paths.data_dir().join("ok").to_str().unwrap(),
            )
        })
        .unwrap();
        service
            .backup_now(&f.db, &f.paths, "2026-11-19T10:00:00.000Z")
            .unwrap();
        assert_eq!(service.status(&f.db, &f.paths).unwrap().last_error, None);
    }

    #[test]
    fn the_folder_must_be_absolute_and_writable() {
        let f = fixture();
        f.db.with(|conn| {
            assert!(set_folder(conn, &f.paths, Some(Path::new("relative/dir"))).is_err());
            assert!(set_folder(conn, &f.paths, Some(Path::new(""))).is_err());
            assert!(set_folder(conn, &f.paths, Some(&f.paths.images_dir())).is_err());

            let chosen = f.paths.data_dir().join("my backups");
            set_folder(conn, &f.paths, Some(&chosen))?;
            assert_eq!(folder(conn, &f.paths)?, chosen);
            assert!(chosen.is_dir());

            set_folder(conn, &f.paths, None)?;
            assert_eq!(folder(conn, &f.paths)?, default_folder(&f.paths));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn the_folder_cannot_be_inside_a_backup_or_a_temporary_folder() {
        let f = fixture();
        let root = f.paths.data_dir().join("shared");
        f.db.with(|conn| {
            for inside in [
                root.join("backup-20261119-210000-123").join("sub"),
                root.join("manual-20261119-210000-123-pc")
                    .join("a")
                    .join("b"),
                root.join("safety-20261119-210000-123"),
                root.join(".tmp-backup-20261119-210000-123"),
                root.join(".tmp-whatever").join("sub"),
            ] {
                assert!(
                    set_folder(conn, &f.paths, Some(&inside)).is_err(),
                    "{}",
                    inside.display()
                );
                assert!(
                    !inside.exists(),
                    "nothing was created: {}",
                    inside.display()
                );
            }
            // A folder merely named like a backup prefix is fine.
            set_folder(conn, &f.paths, Some(&root.join("backups-do-pc")))?;
            Ok(())
        })
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_link_into_a_backup_does_not_get_past_the_folder_check() {
        let f = fixture();
        let root = f.paths.data_dir().join("shared");
        let real = root.join("backup-20261119-210000-123");
        fs::create_dir_all(&real).unwrap();
        let link = f.paths.data_dir().join("innocent");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        f.db.with(|conn| {
            assert!(set_folder(conn, &f.paths, Some(&link)).is_err());
            assert!(set_folder(conn, &f.paths, Some(&link.join("sub"))).is_err());
            assert!(set_folder(conn, &f.paths, Some(&link.join("a").join("b"))).is_err());
            Ok(())
        })
        .unwrap();
        // Refusing must not leave empty folders inside the backup.
        assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
    }

    #[test]
    fn a_new_folder_that_is_a_plain_subfolder_of_the_old_one_is_not_moved_into() {
        let f = fixture();
        let service = BackupService::default();
        let old = default_folder(&f.paths);
        let info = create(
            &f.db,
            &f.paths,
            &old,
            BackupKind::Backup,
            "2026-11-19T10:00:00.000Z",
            false,
        )
        .unwrap();
        // Allowed by `set_folder` (not inside a backup), but a move would delete the copy
        // together with the original, so the relocation refuses it.
        service
            .change_folder(&f.db, &f.paths, Some(&old.join("nested")))
            .unwrap();

        assert!(service.relocate_left_behind(&f.db, &f.paths, true).is_err());
        snapshot::verify(&old.join(&info.name)).unwrap();
    }

    #[test]
    fn a_logged_delete_removes_the_tree_and_tolerates_a_missing_one() {
        let f = fixture();
        let tree = f.paths.data_dir().join("tree");
        fs::create_dir_all(tree.join("a")).unwrap();
        fs::write(tree.join("a").join("file"), b"x").unwrap();
        remove_dir_logged(&tree);
        assert!(!tree.exists());
        remove_dir_logged(&tree);
    }

    #[test]
    fn the_tag_keeps_accented_letters_as_plain_ones() {
        assert_eq!(slug("Máquina"), Some("maquina".into()));
        assert_eq!(slug("Ação-PC Été"), Some("acao-pc-ete".into()));
        assert_eq!(slug("Çãõ"), Some("cao".into()));
        // Letters with no plain form still separate words instead of vanishing.
        assert_eq!(slug("日本-pc"), Some("pc".into()));
    }

    #[test]
    fn accents_written_as_separate_marks_are_dropped_too() {
        // "Máquina" as `a` + U+0301, the way macOS and some keyboards write it.
        assert_eq!(slug("Ma\u{301}quina"), Some("maquina".into()));
        assert_eq!(slug("A\u{327}a\u{303}o"), Some("aao".into()));
    }

    #[test]
    fn long_host_names_that_start_alike_get_different_tags() {
        let a = slug("estacao-de-trabalho-do-escritorio-a").unwrap();
        let b = slug("estacao-de-trabalho-do-escritorio-b").unwrap();
        assert_ne!(a, b);
        assert!(a.len() <= MAX_TAG && b.len() <= MAX_TAG);
        assert!(parse(&format!("backup-20261119-210000-123-{a}")).is_some());
        // The same name always gives the same tag, and a short one is left as it is.
        assert_eq!(slug("estacao-de-trabalho-do-escritorio-a").unwrap(), a);
        assert_eq!(slug("desktop-abc"), Some("desktop-abc".into()));
        assert_eq!(slug(&"x".repeat(MAX_TAG)), Some("x".repeat(MAX_TAG)));
    }

    #[test]
    fn the_macos_local_host_name_wins_over_the_network_host_name() {
        // `gethostname()` follows the network on a Mac ("air-3.lan", "Air.local"); the
        // LocalHostName does not.
        assert_eq!(
            tag_from(Some("MacBook-Air"), "air-3.lan"),
            Some("macbook-air".into())
        );
        assert_eq!(tag_from(None, "Desktop.local"), Some("desktop".into()));
        assert_eq!(tag_from(Some("  "), "Desktop"), Some("desktop".into()));
        assert_eq!(tag_from(Some("???"), "???"), None);
    }

    #[test]
    fn backup_names_round_trip() {
        assert_eq!(stamp("2026-11-19T21:00:00.123Z"), "20261119-210000-123");
        assert_eq!(
            parse_name("backup-20261119-210000-123"),
            Some(BackupKind::Backup)
        );
        assert_eq!(
            parse_name("safety-20261119-210000-123"),
            Some(BackupKind::Safety)
        );
        assert_eq!(parse_name("backup-20261119-210000-12"), None);
        assert_eq!(parse_name(".tmp-backup-20261119-210000-123"), None);
    }

    #[test]
    fn names_may_carry_a_computer_tag_and_old_untagged_names_still_read() {
        let parsed = parse("manual-20261119-210000-123-desktop-a1").unwrap();
        assert_eq!(parsed.kind, BackupKind::Manual);
        assert_eq!(parsed.tag.as_deref(), Some("desktop-a1"));
        assert_eq!(parse("backup-20261119-210000-123").unwrap().tag, None);
        for bad in [
            "backup-20261119-210000-123-",
            "backup-20261119-210000-123--x",
            "backup-20261119-210000-123-X",
            "backup-20261119-210000-123-a-",
            "backup-20261119-210000-123x",
            "backup-20261119-210000-123-a_b",
            "backup-20261119-210000-123-abcdefghijklmnopqrstuvwxyz",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_tag_comes_from_the_host_name_as_a_plain_slug() {
        assert_eq!(slug("DESKTOP-ABC12").as_deref(), Some("desktop-abc12"));
        // A network domain is not part of the name: it changes with the network.
        assert_eq!(slug("MacBook-Pro.local").as_deref(), Some("macbook-pro"));
        assert_eq!(slug("macbook-pro.lan").as_deref(), Some("macbook-pro"));
        assert_eq!(
            slug("  Mac_Book Pro (2)").as_deref(),
            Some("mac-book-pro-2")
        );
        assert_eq!(slug("Máquina").as_deref(), Some("maquina"));
        assert_eq!(slug("a".repeat(40).as_str()).unwrap().len(), MAX_TAG);
        assert_eq!(slug("---"), None);
        assert_eq!(slug(""), None);
        // Too long, with a hyphen where it is cut: still a valid tag, never two hyphens in a row.
        let cut = slug(&format!("{}-b", "a".repeat(MAX_TAG - 6))).unwrap();
        assert!(cut.len() <= MAX_TAG && !cut.contains("--") && !cut.ends_with('-'));
        // Every slug is a name the parser accepts.
        let tag = slug("Meu PC #1").unwrap();
        assert!(parse(&format!("backup-20261119-210000-123-{tag}")).is_some());
    }

    #[test]
    fn computers_sharing_one_folder_rotate_only_their_own_backups() {
        let f = fixture();
        let folder = default_folder(&f.paths);
        as_computer("laptop");
        for day in 1..=3 {
            backup(&f, &format!("2026-11-{day:02}T09:00:00.000Z"));
        }
        // An unfinished backup of the laptop, and an old untagged one.
        fs::create_dir_all(folder.join(".tmp-backup-20261101-000000-000-laptop")).unwrap();
        fs::create_dir_all(folder.join(".tmp-backup-20261101-000000-000")).unwrap();
        as_computer("desktop");
        for day in 4..=12 {
            backup(&f, &format!("2026-11-{day:02}T10:00:00.000Z"));
        }
        let all = list(&folder);
        let by = |tag: &str| {
            all.iter()
                .filter(|b| b.computer.as_deref() == Some(tag))
                .count()
        };
        assert_eq!(by("desktop"), 7, "the desktop keeps its own newest 7");
        assert_eq!(by("laptop"), 3, "the laptop's backups are not touched");
        assert!(
            all.iter()
                .filter(|b| b.computer.as_deref() == Some("laptop"))
                .all(|b| b.other_computer)
        );
        assert!(
            all.iter()
                .filter(|b| b.computer.as_deref() == Some("desktop"))
                .all(|b| !b.other_computer)
        );
        assert!(
            folder
                .join(".tmp-backup-20261101-000000-000-laptop")
                .exists()
        );
        assert!(folder.join(".tmp-backup-20261101-000000-000").exists());
        assert_eq!(all[0].name, "backup-20261112-100000-000-desktop");
    }

    #[cfg(unix)]
    #[test]
    fn old_untagged_and_garbage_temporary_folders_are_cleaned_but_other_computers_are_not() {
        let f = fixture();
        let folder = default_folder(&f.paths);
        fs::create_dir_all(&folder).unwrap();
        let long_ago = std::time::SystemTime::now() - Duration::from_secs(3 * 24 * 60 * 60);
        let make = |name: &str, old: bool| {
            let path = folder.join(name);
            fs::create_dir_all(&path).unwrap();
            if old {
                fs::File::open(&path)
                    .unwrap()
                    .set_modified(long_ago)
                    .unwrap();
            }
            path
        };
        let stale_untagged = make(".tmp-backup-20261101-000000-000", true);
        let stale_garbage = make(".tmp-whatever", true);
        let fresh_untagged = make(".tmp-backup-20261102-000000-000", false);
        let stale_other = make(".tmp-backup-20261101-000000-000-laptop", true);
        as_computer("desktop");
        backup(&f, "2026-11-19T10:00:00.000Z");
        assert!(!stale_untagged.exists());
        assert!(!stale_garbage.exists());
        assert!(fresh_untagged.exists(), "could be a build in progress");
        assert!(
            stale_other.exists(),
            "the other computer's is never touched"
        );
    }

    #[test]
    fn untagged_backups_from_before_tags_are_kept_and_listed() {
        let f = fixture();
        let folder = default_folder(&f.paths);
        for day in 1..=8 {
            backup(&f, &format!("2026-11-{day:02}T10:00:00.000Z"));
        }
        assert_eq!(list(&folder).len(), 7, "no tag: rotation as before");
        as_computer("desktop");
        backup(&f, "2026-11-09T10:00:00.000Z");
        let all = list(&folder);
        assert_eq!(all.len(), 8, "the untagged ones stay");
        let old = all.iter().find(|b| b.computer.is_none()).unwrap();
        assert!(!old.other_computer);
    }

    #[test]
    fn another_computers_backup_does_not_make_this_one_not_due_and_can_be_restored() {
        let f = fixture();
        let folder = default_folder(&f.paths);
        as_computer("laptop");
        let theirs = backup(&f, "2026-11-19T10:00:00.000Z");
        as_computer("desktop");
        let service = BackupService::default();
        let made = service
            .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 11:00:00 UTC))
            .unwrap();
        assert_eq!(made.unwrap().computer.as_deref(), Some("desktop"));
        assert!(
            service
                .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 12:00:00 UTC))
                .unwrap()
                .is_none(),
            "its own backup now counts"
        );
        // The laptop's backup can be restored here; the safety backup is tagged with this
        // computer.
        restore(&f.db, &f.paths, &theirs.name, "2026-11-19T13:00:00.000Z").unwrap();
        assert!(folder.join("safety-20261119-130000-000-desktop").is_dir());
    }

    fn make_left_behind(f: &Fixture, service: &BackupService) -> (PathBuf, PathBuf) {
        let old = default_folder(&f.paths);
        for (kind, at) in [
            (BackupKind::Backup, "2026-11-19T10:00:00.000Z"),
            (BackupKind::Safety, "2026-11-19T11:00:00.000Z"),
        ] {
            create(&f.db, &f.paths, &old, kind, at, false).unwrap();
        }
        let new = f._dir.path().join("synced");
        service.change_folder(&f.db, &f.paths, Some(&new)).unwrap();
        (old, new)
    }

    #[test]
    fn each_backup_is_moved_in_its_own_turn_and_a_refused_turn_stops_the_rest() {
        let f = fixture();
        let service = BackupService::default();
        let (old, new) = make_left_behind(&f, &service);
        let names: Vec<String> = list(&old).into_iter().map(|info| info.name).collect();
        assert_eq!(names.len(), 2);

        let turns = std::cell::Cell::new(0);
        let report = relocate::relocate(&old, &new, &names, true, &|work| {
            turns.set(turns.get() + 1);
            // The second turn is refused (say the folder changed meanwhile).
            if turns.get() == 1 {
                work();
            }
        })
        .unwrap();

        assert_eq!(turns.get(), 2, "one turn per backup, not one for all");
        assert_eq!((report.copied, report.removed, report.failed), (1, 1, 1));
        assert!(report.first_failure.is_some());
        assert_eq!(list(&old).len(), 1, "the refused one stays where it was");
    }

    #[test]
    fn changing_the_folder_between_two_backups_stops_the_move_and_keeps_the_rest() {
        let f = fixture();
        let service = BackupService::default();
        let (old, new) = make_left_behind(&f, &service);
        let third = f._dir.path().join("third");
        let turns = std::cell::Cell::new(0);

        let outcome = service
            .relocate_with(&f.db, &f.paths, true, &|| {
                turns.set(turns.get() + 1);
                if turns.get() == 1 {
                    // Between the first backup and the second.
                    service
                        .change_folder(&f.db, &f.paths, Some(&third))
                        .unwrap();
                }
            })
            .unwrap();

        assert_eq!((outcome.report.removed, outcome.report.failed), (1, 1));
        assert_eq!(list(&old).len(), 1, "the second original was not touched");
        assert_eq!(list(&new).len(), 1);
        assert!(list(&third).is_empty());
        // The change made meanwhile decided what is offered: the old folder (`new` here) is
        // the one with the moved backup, and the notice was not overwritten by the move.
        let left = outcome.status.left_behind.map(|left| left.folder);
        assert_eq!(left, Some(new.display().to_string()));
    }

    #[test]
    fn dismissing_the_offer_between_two_backups_stops_the_move() {
        let f = fixture();
        let service = BackupService::default();
        let (old, _) = make_left_behind(&f, &service);
        let turns = std::cell::Cell::new(0);

        let outcome = service
            .relocate_with(&f.db, &f.paths, true, &|| {
                turns.set(turns.get() + 1);
                if turns.get() == 1 {
                    service.forget_left_behind(&f.db).unwrap();
                }
            })
            .unwrap();

        assert_eq!((outcome.report.removed, outcome.report.failed), (1, 1));
        assert_eq!(list(&old).len(), 1);
        assert_eq!(outcome.status.left_behind, None);
    }

    #[test]
    fn the_offer_to_move_survives_a_restart() {
        let f = fixture();
        let (old, new) = make_left_behind(&f, &BackupService::default());

        // A new service has no memory: everything comes from the stored setting.
        let restarted = BackupService::default();
        let notice = restarted.status(&f.db, &f.paths).unwrap().left_behind;
        assert_eq!(
            notice.map(|left| left.folder),
            Some(old.display().to_string())
        );

        let outcome = restarted
            .relocate_left_behind(&f.db, &f.paths, true)
            .unwrap();
        assert_eq!(outcome.report.removed, 2);
        assert_eq!(list(&new).len(), 2);
        assert_eq!(restarted.status(&f.db, &f.paths).unwrap().left_behind, None);
        assert!(list(&old).is_empty());
    }

    #[test]
    fn an_unreadable_old_folder_hides_the_offer_without_forgetting_it() {
        let f = fixture();
        let service = BackupService::default();
        let (old, _) = make_left_behind(&f, &service);
        // Like an unplugged drive or an offline share: the folder can't be read for now.
        let away = f._dir.path().join("away");
        fs::rename(&old, &away).unwrap();

        assert_eq!(service.status(&f.db, &f.paths).unwrap().left_behind, None);

        fs::rename(&away, &old).unwrap();
        let back = service.status(&f.db, &f.paths).unwrap().left_behind;
        assert_eq!(back.map(|left| left.backups), Some(2), "still offered");
    }

    #[test]
    fn forgetting_a_stale_offer_never_erases_a_newer_one() {
        let f = fixture();
        f.db.with(|conn| remember_old_folder(conn, Some(Path::new("/newer"))))
            .unwrap();
        forget_old_folder_if(&f.db, Path::new("/older")).unwrap();
        let stored =
            f.db.with(|conn| get_setting::<String>(conn, KEY_LEFT_BEHIND))
                .unwrap();
        assert_eq!(stored.as_deref(), Some("/newer"));

        forget_old_folder_if(&f.db, Path::new("/newer")).unwrap();
        let stored =
            f.db.with(|conn| get_setting::<String>(conn, KEY_LEFT_BEHIND))
                .unwrap();
        assert_eq!(stored, None);
    }

    #[test]
    fn a_copy_that_failed_for_some_backups_can_be_tried_again() {
        let f = fixture();
        let service = BackupService::default();
        let (old, new) = make_left_behind(&f, &service);
        // Something unusable already sits under the name of one backup in the new folder.
        let blocked = list(&old)[0].name.clone();
        fs::create_dir_all(new.join(&blocked)).unwrap();

        let first = service
            .relocate_left_behind(&f.db, &f.paths, false)
            .unwrap();
        assert_eq!((first.report.copied, first.report.failed), (1, 1));
        assert!(
            first.status.left_behind.is_some(),
            "the offer stays for the retry"
        );

        fs::remove_dir_all(new.join(&blocked)).unwrap();
        let second = service
            .relocate_left_behind(&f.db, &f.paths, false)
            .unwrap();
        assert_eq!(second.report.failed, 0);
        assert_eq!(second.status.left_behind, None, "done: nothing failed");
        assert_eq!(list(&new).len(), 2);
    }

    #[test]
    fn dismissing_the_offer_forgets_it_for_good() {
        let f = fixture();
        let service = BackupService::default();
        let (old, _) = make_left_behind(&f, &service);

        service.forget_left_behind(&f.db).unwrap();

        let restarted = BackupService::default();
        assert_eq!(restarted.status(&f.db, &f.paths).unwrap().left_behind, None);
        assert!(
            restarted
                .relocate_left_behind(&f.db, &f.paths, true)
                .is_err()
        );
        assert_eq!(list(&old).len(), 2, "nothing was deleted");
    }

    #[test]
    fn the_offer_goes_away_once_the_old_folder_has_nothing_of_ours() {
        let f = fixture();
        let service = BackupService::default();
        let (old, _) = make_left_behind(&f, &service);
        for info in list(&old) {
            fs::remove_dir_all(old.join(info.name)).unwrap();
        }
        assert_eq!(service.status(&f.db, &f.paths).unwrap().left_behind, None);
        assert!(service.relocate_left_behind(&f.db, &f.paths, true).is_err());
    }

    #[test]
    fn copying_the_left_behind_backups_keeps_the_originals() {
        let f = fixture();
        let service = BackupService::default();
        let (old, new) = make_left_behind(&f, &service);

        let outcome = service
            .relocate_left_behind(&f.db, &f.paths, false)
            .unwrap();

        assert_eq!(outcome.report.copied, 2);
        assert_eq!(outcome.report.removed, 0);
        assert_eq!(outcome.report.failed, 0);
        assert_eq!(list(&old).len(), 2, "copying never deletes");
        assert_eq!(list(&new).len(), 2);
        assert_eq!(outcome.status.backups.len(), 2);
        assert_eq!(outcome.status.left_behind, None);
        // Verified copies, restorable from the new folder.
        for info in list(&new) {
            snapshot::verify(&new.join(&info.name)).unwrap();
        }
        // The notice is gone, so there is nothing more to move.
        assert!(service.relocate_left_behind(&f.db, &f.paths, true).is_err());
    }

    #[test]
    fn moving_deletes_each_original_only_after_its_copy_is_verified() {
        let f = fixture();
        let service = BackupService::default();
        let (old, new) = make_left_behind(&f, &service);

        let outcome = service.relocate_left_behind(&f.db, &f.paths, true).unwrap();

        assert_eq!(outcome.report.copied, 2);
        assert_eq!(outcome.report.removed, 2);
        assert!(list(&old).is_empty());
        assert_eq!(list(&new).len(), 2);
        assert_eq!(outcome.status.left_behind, None);
    }

    #[test]
    fn a_backup_that_does_not_verify_after_copying_keeps_its_original() {
        let f = fixture();
        let service = BackupService::default();
        let (old, new) = make_left_behind(&f, &service);
        // Corrupt one original's database: its copy can't pass verification.
        let bad = list(&old)
            .into_iter()
            .find(|b| b.kind == BackupKind::Safety)
            .unwrap();
        fs::write(
            old.join(&bad.name).join("gabaritei.sqlite"),
            b"not a database",
        )
        .unwrap();

        let outcome = service.relocate_left_behind(&f.db, &f.paths, true).unwrap();

        assert_eq!(outcome.report.copied, 1);
        assert_eq!(outcome.report.removed, 1);
        assert_eq!(outcome.report.failed, 1);
        assert!(old.join(&bad.name).is_dir(), "the original stays");
        assert!(!new.join(&bad.name).exists(), "no partial copy is left");
        assert!(
            !new.read_dir().unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp-")),
            "no temporary folder is left"
        );
        // What is still in the old folder is reported, and can be tried again.
        assert_eq!(
            outcome.status.left_behind,
            Some(LeftBehind {
                folder: old.display().to_string(),
                backups: 1,
                safety: 1
            })
        );
    }

    #[test]
    fn a_backup_already_in_the_new_folder_is_not_copied_again_but_can_be_removed() {
        let f = fixture();
        let service = BackupService::default();
        let (old, new) = make_left_behind(&f, &service);
        service
            .relocate_left_behind(&f.db, &f.paths, false)
            .unwrap();
        // The copy exists and the notice is gone: go to the default folder and back to get the
        // notice for the originals again.
        service.change_folder(&f.db, &f.paths, None).unwrap();
        service.change_folder(&f.db, &f.paths, Some(&new)).unwrap();
        let outcome = service.relocate_left_behind(&f.db, &f.paths, true).unwrap();
        assert_eq!(outcome.report.copied, 0);
        assert_eq!(outcome.report.already_there, 2);
        assert_eq!(outcome.report.removed, 2);
        assert!(list(&old).is_empty());
        assert_eq!(list(&new).len(), 2);
    }

    #[test]
    fn a_different_backup_with_the_same_name_is_never_overwritten_or_removed() {
        let f = fixture();
        let service = BackupService::default();
        let old = default_folder(&f.paths);
        create(
            &f.db,
            &f.paths,
            &old,
            BackupKind::Backup,
            "2026-11-19T10:00:00.000Z",
            false,
        )
        .unwrap();
        let new = f._dir.path().join("synced");
        // Same name, different content (another computer without a tag, say).
        add_question(&f.db, "q-extra");
        create(
            &f.db,
            &f.paths,
            &new,
            BackupKind::Backup,
            "2026-11-19T10:00:00.000Z",
            false,
        )
        .unwrap();
        service.change_folder(&f.db, &f.paths, Some(&new)).unwrap();

        let outcome = service.relocate_left_behind(&f.db, &f.paths, true).unwrap();

        assert_eq!(outcome.report.failed, 1);
        assert_eq!(outcome.report.removed, 0);
        assert_eq!(list(&old).len(), 1);
        snapshot::verify(&new.join("backup-20261119-100000-000")).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn moving_into_the_same_physical_folder_never_deletes_the_only_copy() {
        let f = fixture();
        let old = default_folder(&f.paths);
        let info = create(
            &f.db,
            &f.paths,
            &old,
            BackupKind::Backup,
            "2026-11-19T10:00:00.000Z",
            false,
        )
        .unwrap();
        // Another spelling of the same folder.
        let alias = f._dir.path().join("alias");
        std::os::unix::fs::symlink(&old, &alias).unwrap();

        let report = relocate::relocate(
            &old,
            &alias,
            std::slice::from_ref(&info.name),
            true,
            &|work| work(),
        )
        .unwrap();

        assert_eq!(report.failed, 1);
        assert_eq!(report.removed, 0);
        snapshot::verify(&old.join(&info.name)).unwrap();
    }

    #[test]
    fn a_new_folder_inside_the_old_one_is_refused() {
        let f = fixture();
        let service = BackupService::default();
        let old = default_folder(&f.paths);
        let info = create(
            &f.db,
            &f.paths,
            &old,
            BackupKind::Backup,
            "2026-11-19T10:00:00.000Z",
            false,
        )
        .unwrap();
        let inside = old.join(&info.name).join("sub");
        // Refused up front by `set_folder` (#109), so nothing is left to move into it.
        assert!(
            service
                .change_folder(&f.db, &f.paths, Some(&inside))
                .is_err()
        );
        assert!(!inside.exists());
        snapshot::verify(&old.join(&info.name)).unwrap();
    }

    #[test]
    fn another_computers_backups_are_left_where_they_are() {
        let f = fixture();
        let service = BackupService::default();
        let old = default_folder(&f.paths);
        as_computer("laptop");
        let theirs = create(
            &f.db,
            &f.paths,
            &old,
            BackupKind::Backup,
            "2026-11-19T09:00:00.000Z",
            false,
        )
        .unwrap();
        as_computer("desktop");
        create(
            &f.db,
            &f.paths,
            &old,
            BackupKind::Backup,
            "2026-11-19T10:00:00.000Z",
            false,
        )
        .unwrap();
        let new = f._dir.path().join("synced");
        service.change_folder(&f.db, &f.paths, Some(&new)).unwrap();

        let outcome = service.relocate_left_behind(&f.db, &f.paths, true).unwrap();

        assert_eq!(outcome.report.removed, 1);
        assert_eq!(outcome.report.left_alone, 1);
        assert!(old.join(&theirs.name).is_dir(), "the laptop's backup stays");
        assert!(!new.join(&theirs.name).exists());
        assert_eq!(outcome.status.left_behind, None);
    }

    #[test]
    fn untagged_backups_are_not_ours_to_move_on_a_tagged_computer() {
        let f = fixture();
        let service = BackupService::default();
        let old = default_folder(&f.paths);
        // Made before tags existed (or by a computer without a tag): it can't be told whose.
        tests::TAG.with(|tag| *tag.borrow_mut() = Some(None));
        let untagged = create(
            &f.db,
            &f.paths,
            &old,
            BackupKind::Backup,
            "2026-11-19T09:00:00.000Z",
            false,
        )
        .unwrap();
        as_computer("desktop");
        create(
            &f.db,
            &f.paths,
            &old,
            BackupKind::Backup,
            "2026-11-19T10:00:00.000Z",
            false,
        )
        .unwrap();
        let new = f._dir.path().join("synced");
        service.change_folder(&f.db, &f.paths, Some(&new)).unwrap();

        let outcome = service.relocate_left_behind(&f.db, &f.paths, true).unwrap();

        assert_eq!(outcome.report.removed, 1);
        assert_eq!(outcome.report.left_alone, 1);
        assert!(
            old.join(&untagged.name).is_dir(),
            "never deleted from the old folder"
        );
        assert!(!new.join(&untagged.name).exists(), "and not copied either");
        assert_eq!(outcome.status.left_behind, None);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_inside_a_backup_is_never_followed_and_extra_files_are_not_copied() {
        let f = fixture();
        let service = BackupService::default();
        let old = default_folder(&f.paths);
        let info = create(
            &f.db,
            &f.paths,
            &old,
            BackupKind::Backup,
            "2026-11-19T10:00:00.000Z",
            false,
        )
        .unwrap();
        let dir = old.join(&info.name);
        fs::write(dir.join("notes.txt"), b"extra").unwrap();
        let new = f._dir.path().join("synced");
        service.change_folder(&f.db, &f.paths, Some(&new)).unwrap();
        let outcome = service
            .relocate_left_behind(&f.db, &f.paths, false)
            .unwrap();
        assert_eq!(outcome.report.copied, 1);
        assert!(!new.join(&info.name).join("notes.txt").exists());

        // Now the database file is a link: not a regular file, so the copy is refused.
        let other = create(
            &f.db,
            &f.paths,
            &old,
            BackupKind::Backup,
            "2026-11-19T11:00:00.000Z",
            false,
        )
        .unwrap();
        let db_file = old.join(&other.name).join("gabaritei.sqlite");
        let real = old.join("real.sqlite");
        fs::rename(&db_file, &real).unwrap();
        std::os::unix::fs::symlink(&real, &db_file).unwrap();
        service.change_folder(&f.db, &f.paths, None).unwrap();
        service.change_folder(&f.db, &f.paths, Some(&new)).unwrap();
        let outcome = service.relocate_left_behind(&f.db, &f.paths, true).unwrap();
        assert_eq!(outcome.report.failed, 1);
        assert!(old.join(&other.name).is_dir());
    }
}
