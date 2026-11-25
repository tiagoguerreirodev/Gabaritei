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

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, mpsc};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::ai::settings::{KEY_CLI_PATHS, KEY_REMNOTE, KEY_ROLES};
use crate::appearance;
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
    appearance::KEY,
    crate::shortcuts::KEY,
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
    /// pt-BR reason the last automatic or manual backup failed, until one succeeds.
    pub last_error: Option<String>,
    /// Set while the old folder still holds this computer's backups after a folder change. It is
    /// remembered across restarts until the backups are copied or moved (a copy or move with
    /// failures keeps it), the user dismisses it, the folder is changed again, or the old folder
    /// holds none of this computer's backups.
    pub left_behind: Option<LeftBehind>,
    pub listing: BackupListing,
}

/// Whether the Dados page can show the backup list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
#[ts(export)]
pub enum BackupListing {
    /// Backups, newest first.
    Ready { backups: Vec<BackupInfo> },
    /// The folder answered, but its manifests are still taking too long to read.
    Slow,
    /// The folder could not be opened or did not answer in time.
    FolderUnavailable,
}

/// How long reading the manifests of a healthy backup folder may take: a cloud-synced folder may
/// have to download them first, so this is longer than the deadline of a plain `read_dir`.
const MANIFEST_READ_TIMEOUT: Duration = Duration::from_secs(if cfg!(test) { 60 } else { 15 });

/// The probe key of the slow manifest listing of `folder`, apart from the folder's own.
fn manifest_key(folder: &Path) -> PathBuf {
    let mut key = folder.as_os_str().to_owned();
    key.push("\u{0}manifests");
    PathBuf::from(key)
}

/// A stalled write check must leave the configured folder's read probes available.
fn candidate_key(folder: &Path) -> PathBuf {
    let mut key = folder.as_os_str().to_owned();
    // NUL keeps this key distinct from filesystem paths and manifest probe keys.
    key.push("\u{0}candidate");
    PathBuf::from(key)
}

/// What a quick look at the backup folder found.
enum FolderState {
    /// It can be listed.
    Present,
    /// It does not exist yet (or any more) but its parent does: nothing wrong, it is created
    /// on first use.
    Missing,
    /// It can't be read, or its parent is gone too (a removed drive or volume).
    Unavailable,
}

fn folder_state(folder: &Path) -> FolderState {
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
fn mount_point_missing(folder: &Path) -> bool {
    folder
        .strip_prefix("/Volumes")
        .ok()
        .and_then(|rest| rest.components().next())
        .is_some_and(|name| !Path::new("/Volumes").join(name).exists())
}

#[cfg(not(target_os = "macos"))]
fn mount_point_missing(_folder: &Path) -> bool {
    false
}

fn folder_unavailable() -> AppError {
    AppError::Invalid(
        "A pasta dos backups não está acessível agora (um disco desconectado ou uma pasta de rede fora do ar). Conecte-a de novo ou escolha outra pasta."
            .into(),
    )
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
    /// The old folder can't be read now (a drive that is unplugged, a share that is offline, a
    /// folder that was deleted). The offer is kept, since the drive may come back, but there is
    /// nothing to count or move: the user can only dismiss it.
    pub unavailable: bool,
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

/// UTC stamp (as in backup names) from which the previous tag is no longer trusted. #126 (the
/// current derivation) was merged at 2026-11-21 15:57 UTC; the cutoff is a day later because a
/// computer keeps making old-style backups until it runs a new build.
const PREVIOUS_TAG_UNTIL: &str = "20261122-160000-000";

/// The tag this computer's backups carried before the derivation changed (#126): builds from
/// `main` since the tags appeared slugged the network host name (`gethostname()`) up to the
/// first dot, with ASCII letters and digits only (`Máquina.local` → `m-quina`), cut at
/// [`MAX_TAG`]. Backups with it, made before [`PREVIOUS_TAG_UNTIL`], still belong to this
/// computer, so they are rotated, counted for the daily check and moved with the rest, instead
/// of looking like another computer's.
///
/// This trusts a tag rebuilt from today's network host name, which can differ from the one used
/// back then (`air-3.lan` one day, `Air.local` the next): such backups stay unrecognized, and
/// the cutoff keeps any later backup with that tag (another computer's) from being claimed.
fn previous_tag() -> Option<String> {
    #[cfg(test)]
    {
        tests::PREVIOUS.with(|tag| tag.borrow().clone())
    }
    #[cfg(not(test))]
    {
        static TAG: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        TAG.get_or_init(|| previous_tag_from(&gethostname::gethostname().to_string_lossy()))
            .clone()
    }
}

#[cfg_attr(test, allow(dead_code))] // tests set the previous tag directly
fn previous_tag_from(host_name: &str) -> Option<String> {
    ascii_slug(host_name.split('.').next().unwrap_or_default())
}

/// The first tag derivations: ASCII letters and digits lowercased, other runs one hyphen, cut
/// at [`MAX_TAG`] with no hash.
fn ascii_slug(text: &str) -> Option<String> {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.truncate(MAX_TAG);
    let out = out.trim_end_matches('-');
    (!out.is_empty()).then(|| out.to_owned())
}

/// Whether a backup tagged `tag` (`None`: untagged) made at `stamp` is this computer's by name:
/// it carries this computer's tag, or the [`previous_tag`] from before [`PREVIOUS_TAG_UNTIL`],
/// or it is untagged on a computer with no tag.
fn owns_tag(tag: Option<&str>, stamp: &str) -> bool {
    match tag {
        Some(tag) => {
            computer_tag().as_deref() == Some(tag)
                || (stamp < PREVIOUS_TAG_UNTIL && previous_tag().as_deref() == Some(tag))
        }
        None => computer_tag().is_none(),
    }
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
    /// The folder name this was read from.
    name: String,
    kind: BackupKind,
    /// `YYYYMMDD-HHMMSS-mmm`, UTC.
    stamp: String,
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
        name: name.to_owned(),
        kind,
        stamp: stamp.to_owned(),
        tag: tag.map(str::to_owned),
    })
}

/// The kind a folder name says it is, if it is a backup name at all.
fn parse_name(name: &str) -> Option<BackupKind> {
    parse(name).map(|parsed| parsed.kind)
}

/// Whether a backup tagged `tag` was made by another computer than the one running.
fn is_other_computer(tag: Option<&str>, stamp: &str) -> bool {
    tag.is_some() && !owns_tag(tag, stamp)
}

/// Whether a backup in a shared folder is not this computer's to copy or delete: another
/// computer's, or (on a computer that has a tag) an untagged one, which can't be told apart.
/// Rotation leaves these alone for the same reason.
fn leave_alone(backup: &ParsedName) -> bool {
    !owns_tag(backup.tag.as_deref(), &backup.stamp)
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

/// Checks that the chosen folder can be used and returns the path to store.
fn check_chosen_folder(chosen: &Path, images_dir: &Path) -> AppResult<String> {
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
    try_list(folder).unwrap_or_default()
}

/// Backups in `folder`, newest first, ignoring incomplete backups. Returns `None` if the
/// directory cannot be opened.
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
                other_computer: is_other_computer(parsed.tag.as_deref(), &parsed.stamp),
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
fn try_list_names(folder: &Path) -> Option<Vec<ParsedName>> {
    let entries = fs::read_dir(folder).ok()?;
    Some(
        entries
            .filter_map(Result::ok)
            // `file_type` does not follow links: a link named like a backup is not one.
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| parse(&entry.file_name().into_string().ok()?))
            .collect(),
    )
}

/// The notice for the backups `left` in `old`, if there are any.
fn left_behind(old: &Path, left: &[&ParsedName]) -> Option<LeftBehind> {
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
fn remember_old_folder(conn: &Connection, old: Option<&Path>) -> AppResult<()> {
    match old {
        Some(old) => set_setting(conn, KEY_LEFT_BEHIND, &old.display().to_string()),
        None => crate::db::put_raw_setting(conn, KEY_LEFT_BEHIND, None),
    }
}

/// Why `new` can't receive the old folder's backups, if it can't. The new folder may not exist
/// (a removed folder or drive), so its deepest existing ancestor is looked at: if that is inside
/// the old folder, so is the new one. `symlink_metadata` does not follow links, so a link whose
/// target is missing is that ancestor too; one that can't be resolved can't be shown to be
/// outside the old folder, so it is refused (with its own message: it may lead anywhere).
fn new_folder_problem(old: &Path, new: &Path) -> Option<&'static str> {
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

/// What looking at the remembered old folder found.
enum OldFolder {
    /// It is the current folder now.
    Current,
    /// Its backups, by name.
    Listed(Vec<ParsedName>),
    /// It can't be read (gone, unplugged, no permission).
    Unreadable,
}

/// Looks at the old folder: whether it is the current one, and what it holds.
fn inspect_old_folder(old: &Path, current: &Path) -> OldFolder {
    if same_folder(old, current) {
        OldFolder::Current
    } else {
        try_list_names(old).map_or(OldFolder::Unreadable, OldFolder::Listed)
    }
}

/// Runs backup folder checks with a deadline. A drive that is asleep or a share that is offline
/// can block filesystem calls for minutes. The work runs on its own thread; past the deadline
/// the caller stops waiting and reports the folder as unavailable (a call can wait for another
/// healthy one on the same folder and then run, so at most twice the timeout). A thread stuck in
/// the operating system can't be cancelled, so while one is stuck on a folder, later calls on
/// that folder answer at once instead of starting (and leaking) another.
struct FolderProbe {
    probes: Arc<Probes>,
    next_id: AtomicU64,
    timeout: Duration,
}

/// The probes that are running or stuck, by folder.
type Probes = (Mutex<HashMap<PathBuf, Probe>>, Condvar);

struct Probe {
    id: u64,
    stuck: bool,
}

/// Frees the folder's entry when the worker ends, however it ends (a panic included).
struct FreeProbe {
    probes: Arc<Probes>,
    path: PathBuf,
    id: u64,
}

impl Drop for FreeProbe {
    fn drop(&mut self) {
        let (mutex, wake) = &*self.probes;
        let mut probes = lock(mutex);
        if probes
            .get(&self.path)
            .is_some_and(|probe| probe.id == self.id)
        {
            probes.remove(&self.path);
        }
        wake.notify_all();
    }
}

impl Default for FolderProbe {
    fn default() -> Self {
        // Tests don't depend on the clock: a loaded machine can take longer than 3 s to read a
        // folder, and they make a folder "stuck" explicitly instead.
        Self::with_timeout(Duration::from_secs(if cfg!(test) { 60 } else { 3 }))
    }
}

impl FolderProbe {
    fn with_timeout(timeout: Duration) -> Self {
        Self {
            probes: Arc::default(),
            next_id: AtomicU64::new(1),
            timeout,
        }
    }

    /// The result of `work` on `path`, or `None` when it didn't finish in time (or an earlier
    /// call on the same path is still stuck).
    fn run<T: Send + 'static>(
        &self,
        path: &Path,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        self.run_for(path, self.timeout, work)
    }

    /// [`Self::run`] with its own deadline, for work that is slow when healthy (reading many
    /// manifests, which a cloud-synced folder may have to download first).
    fn run_for<T: Send + 'static>(
        &self,
        path: &Path,
        timeout: Duration,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        let deadline = Instant::now() + timeout;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (mutex, wake) = &*self.probes;
        {
            let mut probes = lock(mutex);
            loop {
                match probes.get(path) {
                    None => {
                        probes.insert(path.to_path_buf(), Probe { id, stuck: false });
                        break;
                    }
                    Some(other) if other.stuck => return None,
                    // A healthy probe of another call on this folder: wait for it.
                    Some(_) => {
                        let left = deadline.saturating_duration_since(Instant::now());
                        if left.is_zero() {
                            return None;
                        }
                        probes = wake
                            .wait_timeout(probes, left)
                            .unwrap_or_else(PoisonError::into_inner)
                            .0;
                    }
                }
            }
        }
        let (done, result) = mpsc::channel();
        let free = FreeProbe {
            probes: self.probes.clone(),
            path: path.to_path_buf(),
            id,
        };
        // Tests fake the computer tag per thread; the worker must see the same one.
        #[cfg(test)]
        let overrides = tests::overrides();
        let spawned = std::thread::Builder::new().spawn(move || {
            // Held for the whole run, so a panic in `work` still frees the entry.
            let free = free;
            #[cfg(test)]
            tests::apply(overrides);
            let value = work();
            // Free the folder before the caller can see the result: otherwise a caller that
            // answers at once may still find the entry occupied (and a test of that flaked).
            drop(free);
            let _ = done.send(value);
        });
        if spawned.is_err() {
            // The closure (and the guard in it) was dropped: the entry is already free.
            return None;
        }
        match result.recv_timeout(timeout) {
            Ok(value) => Some(value),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let mut probes = lock(mutex);
                if let Some(probe) = probes.get_mut(path).filter(|probe| probe.id == id) {
                    probe.stuck = true;
                }
                // Callers waiting on this folder can answer now instead of at their deadline.
                wake.notify_all();
                None
            }
            // The work panicked; the guard freed the entry.
            Err(mpsc::RecvTimeoutError::Disconnected) => None,
        }
    }

    /// Test helper: the folder is stuck as if a read had hung, with no clock involved.
    #[cfg(test)]
    fn mark_stuck(&self, path: &Path) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        lock(&self.probes.0).insert(path.to_path_buf(), Probe { id, stuck: true });
    }

    /// [`inspect_old_folder`] within the deadline; `None` when it timed out. The same folder
    /// is answered without touching the disk.
    fn inspect(&self, old: &Path, current: &Path) -> Option<OldFolder> {
        if old == current {
            return Some(OldFolder::Current);
        }
        let (path, current) = (old.to_path_buf(), current.to_path_buf());
        self.run(old, move || inspect_old_folder(&path, &current))
    }
}

/// The remembered old folder and what it still holds of this computer's, when there is
/// something to offer. A stale memory (the folder is the current one now, or has nothing of ours
/// left) is forgotten here. An unreadable folder stays remembered and is offered as
/// `unavailable`, so it can be dismissed. Also returns every backup name found there (empty
/// when unavailable), so a move works from what the notice counted and reads the folder once.
fn pending_old_folder(
    db: &Database,
    probe: &FolderProbe,
    current: &Path,
) -> AppResult<Option<(PathBuf, LeftBehind, Vec<ParsedName>)>> {
    let Some(old) = db.with(|conn| get_setting::<String>(conn, KEY_LEFT_BEHIND))? else {
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
    /// A copy or move of the old folder's backups is running. Its turns share the `busy` lock
    /// with everything else, so a second one would interleave with it and report failures that
    /// are not real.
    relocating: AtomicBool,
    /// Bounds filesystem checks that may touch an offline share.
    probe: FolderProbe,
}

/// Marks a relocation as running until dropped.
struct Relocating<'a>(&'a AtomicBool);

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

    fn remember<T>(&self, result: &AppResult<T>) {
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
            let chosen = get_setting::<String>(conn, KEY_FOLDER)?;
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

    fn change_folder_with(
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
                    Some(folder) => set_setting(tx, KEY_FOLDER, folder)?,
                    None => crate::db::put_raw_setting(tx, KEY_FOLDER, None)?,
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
    fn relocate_with(
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
                            _ => Some(is_due(&path, now)),
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

/// `2026-11-19T10:00:00.000Z` from a backup name's stamp `20261119-100000-000` (UTC), so it
/// compares with [`crate::clock::format`] output.
fn stamp_to_iso(stamp: &str) -> String {
    format!(
        "{}-{}-{}T{}:{}:{}.{}Z",
        &stamp[0..4],
        &stamp[4..6],
        &stamp[6..8],
        &stamp[9..11],
        &stamp[11..13],
        &stamp[13..15],
        &stamp[16..19]
    )
}

/// Whether the newest backup of this computer is older than a day (or there is none).
/// A backup another computer made into a shared folder doesn't count. Read from the folder
/// names alone (the stamp has the time of the backup), so it stays cheap on a cloud-synced
/// folder whose manifests would have to be downloaded.
fn is_due(folder: &Path, now: OffsetDateTime) -> bool {
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

    thread_local! {
        /// Overrides [`previous_tag`] for the current test.
        pub(super) static PREVIOUS: std::cell::RefCell<Option<String>> =
            const { std::cell::RefCell::new(None) };
    }

    /// The per-thread test overrides, to copy onto a probe's worker thread.
    pub(super) fn overrides() -> (Option<Option<String>>, Option<String>) {
        (
            TAG.with(|tag| tag.borrow().clone()),
            PREVIOUS.with(|tag| tag.borrow().clone()),
        )
    }

    pub(super) fn apply((tag, previous): (Option<Option<String>>, Option<String>)) {
        TAG.with(|current| *current.borrow_mut() = tag);
        PREVIOUS.with(|current| *current.borrow_mut() = previous);
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
        BackupService::default()
            .change_folder(&f.db, &f.paths, Some(&chosen))
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
        assert_eq!(
            status.listing,
            BackupListing::Ready {
                backups: Vec::new()
            }
        );
        assert_eq!(
            status.left_behind,
            Some(LeftBehind {
                folder: old.display().to_string(),
                backups: 2,
                safety: 1,
                unavailable: false,
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
            set_setting(
                conn,
                appearance::KEY,
                &"dark and xl from the other computer",
            )?;
            set_setting(conn, "other", &"from the backup")
        })
        .unwrap();
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        f.db.with(|conn| {
            set_setting(conn, KEY_CLI_PATHS, &"mac-path")?;
            set_setting(conn, KEY_REMNOTE, &"http://localhost/mcp")?;
            set_setting(conn, KEY_ROLES, &"roles of this computer")?;
            set_setting(conn, appearance::KEY, &"light and m of this computer")?;
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
                get_setting::<String>(conn, appearance::KEY)?.as_deref(),
                Some("light and m of this computer")
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
        let service = BackupService::default();
        let change = |chosen| service.change_folder(&f.db, &f.paths, chosen);
        assert!(change(Some(Path::new("relative/dir"))).is_err());
        assert!(change(Some(Path::new(""))).is_err());
        let images = f.paths.images_dir();
        assert!(change(Some(&images)).is_err());

        let file = f.paths.data_dir().join("file");
        fs::write(&file, []).unwrap();
        let error = change(Some(&file)).unwrap_err();
        assert!(error.to_string().contains("Não foi possível gravar"));

        let chosen = f.paths.data_dir().join("my backups");
        assert_eq!(
            change(Some(&chosen)).unwrap().folder,
            chosen.display().to_string()
        );
        assert!(chosen.is_dir());

        let status = change(None).unwrap();
        assert!(status.is_default_folder);
        assert_eq!(
            status.folder,
            default_folder(&f.paths).display().to_string()
        );
    }

    #[test]
    fn the_folder_cannot_be_inside_a_backup_or_a_temporary_folder() {
        let f = fixture();
        let service = BackupService::default();
        let root = f.paths.data_dir().join("shared");
        for inside in [
            root.join("backup-20261119-210000-123").join("sub"),
            root.join("manual-20261119-210000-123-pc")
                .join("a")
                .join("b"),
            root.join("safety-20261119-210000-123"),
            root.join(".tmp-backup-20261119-210000-123"),
            root.join(".tmp-whatever").join("sub"),
        ] {
            let error = service
                .change_folder(&f.db, &f.paths, Some(&inside))
                .unwrap_err();
            assert!(error.to_string().contains("dentro de um backup"));
            assert!(
                !inside.exists(),
                "nothing was created: {}",
                inside.display()
            );
        }
        // A folder merely named like a backup prefix is fine.
        service
            .change_folder(&f.db, &f.paths, Some(&root.join("backups-do-pc")))
            .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_link_into_a_backup_does_not_get_past_the_folder_check() {
        let f = fixture();
        let service = BackupService::default();
        let root = f.paths.data_dir().join("shared");
        let real = root.join("backup-20261119-210000-123");
        fs::create_dir_all(&real).unwrap();
        let link = f.paths.data_dir().join("innocent");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        for inside in [&link, &link.join("sub"), &link.join("a").join("b")] {
            let error = service
                .change_folder(&f.db, &f.paths, Some(inside))
                .unwrap_err();
            assert!(error.to_string().contains("dentro de um backup"));
        }
        // Refusing must not leave empty folders inside the backup.
        assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_link_into_a_backup_creates_nothing_either() {
        let f = fixture();
        let service = BackupService::default();
        let root = f.paths.data_dir().join("shared");
        let real = root.join("backup-20261119-210000-123");
        fs::create_dir_all(&real).unwrap();
        // The link's target does not exist yet.
        let link = f.paths.data_dir().join("dangling");
        std::os::unix::fs::symlink(real.join("missing"), &link).unwrap();
        let error = service
            .change_folder(&f.db, &f.paths, Some(&link.join("sub")))
            .unwrap_err();
        assert!(error.to_string().contains("Não foi possível gravar"));
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
        // Accepted as a backup folder, but a move would delete the copy
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
    fn the_previous_tag_is_the_first_label_of_the_host_name_with_ascii_only() {
        assert_eq!(previous_tag_from("mac").as_deref(), Some("mac"));
        assert_eq!(previous_tag_from("Mac.local").as_deref(), Some("mac"));
        // Accents were dropped to hyphens back then.
        assert_eq!(previous_tag_from("Máquina.lan").as_deref(), Some("m-quina"));
        // Cut at the limit, without the hash of the current derivation.
        assert_eq!(
            previous_tag_from(&"a".repeat(40)),
            Some("a".repeat(MAX_TAG))
        );
        assert_eq!(previous_tag_from("???"), None);
    }

    #[test]
    fn backups_with_an_old_style_tag_of_this_computer_are_still_its_own() {
        let f = fixture();
        let folder = default_folder(&f.paths);
        let service = BackupService::default();
        // Made by builds that slugged the host name the old way.
        as_computer("m-quina");
        for day in 1..=7 {
            backup(&f, &format!("2026-11-{day:02}T10:00:00.000Z"));
        }
        as_computer("laptop");
        backup(&f, "2026-11-08T10:00:00.000Z");
        // The tag derivation changed.
        as_computer("maquina");
        PREVIOUS.with(|tag| *tag.borrow_mut() = Some("m-quina".to_owned()));

        let all = list(&folder);
        let old_style = |b: &&BackupInfo| b.computer.as_deref() == Some("m-quina");
        assert_eq!(all.iter().filter(old_style).count(), 7);
        assert!(all.iter().filter(old_style).all(|b| !b.other_computer));
        assert!(
            all.iter()
                .filter(|b| b.computer.as_deref() == Some("laptop"))
                .all(|b| b.other_computer),
            "another computer's tag is never claimed"
        );

        // Its own, so they count for the daily check...
        assert!(
            service
                .backup_if_due(&f.db, &f.paths, datetime!(2026-11-07 20:00:00 UTC))
                .unwrap()
                .is_none(),
            "the old-style backup of that day counts"
        );
        // ... and they rotate together with the new ones: the oldest go first.
        backup(&f, "2026-11-09T10:00:00.000Z");
        let all = list(&folder);
        let own = all.iter().filter(|b| !b.other_computer).count();
        assert_eq!(own, 7, "seven kept in all, old-style and new");
        assert!(!all.iter().any(|b| b.name.starts_with("backup-20261101")));
        assert_eq!(all.iter().filter(|b| b.other_computer).count(), 1);
    }

    #[test]
    fn the_old_style_tag_is_only_trusted_before_the_new_derivation_shipped() {
        let f = fixture();
        let folder = default_folder(&f.paths);
        as_computer("m-quina");
        backup(&f, "2026-11-22T15:00:00.000Z");
        // Another computer whose tag happens to equal this one's old tag, made later.
        backup(&f, "2026-11-22T17:00:00.000Z");
        as_computer("maquina");
        PREVIOUS.with(|tag| *tag.borrow_mut() = Some("m-quina".to_owned()));

        let all = list(&folder);
        let at = |stamp: &str| all.iter().find(|b| b.name.contains(stamp)).unwrap();
        assert!(!at("20261122-150000").other_computer);
        assert!(
            at("20261122-170000").other_computer,
            "made after the cutoff"
        );
    }

    #[test]
    fn old_style_backups_of_this_computer_are_moved_with_the_rest() {
        let f = fixture();
        let service = BackupService::default();
        let old = default_folder(&f.paths);
        as_computer("m-quina");
        backup(&f, "2026-11-19T10:00:00.000Z");
        as_computer("laptop");
        backup(&f, "2026-11-19T11:00:00.000Z");
        as_computer("maquina");
        PREVIOUS.with(|tag| *tag.borrow_mut() = Some("m-quina".to_owned()));
        let new = f._dir.path().join("new");
        service.change_folder(&f.db, &f.paths, Some(&new)).unwrap();

        let outcome = service.relocate_left_behind(&f.db, &f.paths, true).unwrap();

        assert_eq!((outcome.report.removed, outcome.report.left_alone), (1, 1));
        assert_eq!(list(&new).len(), 1);
        assert_eq!(list(&old).len(), 1, "only the laptop's stays");
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
    fn a_second_relocation_is_refused_while_one_runs_and_allowed_after_it() {
        let f = fixture();
        let service = BackupService::default();
        make_left_behind(&f, &service);
        let refused = std::cell::RefCell::new(None);

        let outcome = service
            .relocate_with(&f.db, &f.paths, true, &|| {
                if refused.borrow().is_none() {
                    *refused.borrow_mut() =
                        Some(service.relocate_left_behind(&f.db, &f.paths, true));
                }
            })
            .unwrap();

        let refused = refused.into_inner().unwrap();
        assert!(
            refused
                .as_ref()
                .is_err_and(|e| e.to_string().contains("em andamento")),
            "{refused:?}"
        );
        assert_eq!((outcome.report.removed, outcome.report.failed), (2, 0));
        // The flag is released when the first one ends, even if it fails.
        assert!(!service.relocating.load(Ordering::Acquire));
        assert!(service.relocate_left_behind(&f.db, &f.paths, true).is_err());
        assert!(!service.relocating.load(Ordering::Acquire));
    }

    #[test]
    fn a_missing_new_folder_inside_the_old_one_is_still_refused_and_not_created() {
        let f = fixture();
        let service = BackupService::default();
        let old = default_folder(&f.paths);
        backup(&f, "2026-11-19T10:00:00.000Z");
        let nested = old.join("sub").join("deeper");
        service
            .change_folder(&f.db, &f.paths, Some(&nested))
            .unwrap();
        fs::remove_dir_all(old.join("sub")).unwrap();

        assert!(service.relocate_left_behind(&f.db, &f.paths, true).is_err());

        assert!(
            !old.join("sub").exists(),
            "nothing was created inside the old folder"
        );
        assert_eq!(list(&old).len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_new_folder_behind_a_dangling_link_into_the_old_one_is_refused() {
        let f = fixture();
        let service = BackupService::default();
        let old = default_folder(&f.paths);
        backup(&f, "2026-11-19T10:00:00.000Z");
        // `link` points into the old folder at a place that does not exist yet.
        let link = f._dir.path().join("link");
        std::os::unix::fs::symlink(old.join("missing"), &link).unwrap();
        service
            .change_folder(&f.db, &f.paths, Some(&f._dir.path().join("elsewhere")))
            .unwrap();
        f.db.with(|conn| {
            // Chosen while the link still resolved to nothing; set directly, as a stale setting.
            set_setting(conn, KEY_FOLDER, &link.join("sub").display().to_string())
        })
        .unwrap();

        let error = service
            .relocate_left_behind(&f.db, &f.paths, true)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("possível verificar a pasta nova"),
            "{error}"
        );

        assert!(
            !old.join("missing").exists(),
            "nothing was created through the link"
        );
        assert_eq!(list(&old).len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_named_like_a_backup_is_not_a_backup_for_the_notice() {
        let f = fixture();
        let service = BackupService::default();
        let (old, _) = make_left_behind(&f, &service);
        let real = list(&old).remove(0).name;
        std::os::unix::fs::symlink(old.join(&real), old.join("backup-20261101-000000-000"))
            .unwrap();

        let notice = service
            .status(&f.db, &f.paths)
            .unwrap()
            .left_behind
            .unwrap();

        assert_eq!(notice.backups, 2, "the link is not counted");
    }

    #[test]
    fn the_new_folder_is_created_when_the_move_starts() {
        let f = fixture();
        let service = BackupService::default();
        let (_, new) = make_left_behind(&f, &service);
        fs::remove_dir_all(&new).unwrap();

        let outcome = service
            .relocate_left_behind(&f.db, &f.paths, false)
            .unwrap();

        assert_eq!(outcome.report.copied, 2);
        assert_eq!(list(&new).len(), 2);
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
    fn an_unreadable_old_folder_is_offered_as_unavailable_and_can_be_dismissed() {
        let f = fixture();
        let service = BackupService::default();
        let (old, _) = make_left_behind(&f, &service);
        // Like an unplugged drive or an offline share: the folder can't be read for now.
        let away = f._dir.path().join("away");
        fs::rename(&old, &away).unwrap();

        let away_notice = service
            .status(&f.db, &f.paths)
            .unwrap()
            .left_behind
            .unwrap();
        assert!(away_notice.unavailable);
        assert_eq!(away_notice.folder, old.display().to_string());
        let error = service
            .relocate_left_behind(&f.db, &f.paths, true)
            .unwrap_err();
        assert!(error.to_string().contains("não está acessível"), "{error}");

        // The drive comes back: the same offer, now with its backups.
        fs::rename(&away, &old).unwrap();
        let back = service
            .status(&f.db, &f.paths)
            .unwrap()
            .left_behind
            .unwrap();
        assert_eq!((back.backups, back.unavailable), (2, false));

        // Gone for good (or never coming back): the user can still dismiss it.
        fs::rename(&old, &away).unwrap();
        assert!(
            service
                .status(&f.db, &f.paths)
                .unwrap()
                .left_behind
                .is_some()
        );
        service.forget_left_behind(&f.db).unwrap();
        assert_eq!(service.status(&f.db, &f.paths).unwrap().left_behind, None);
    }

    #[test]
    fn a_backup_with_a_broken_manifest_is_counted_fails_to_move_and_keeps_the_offer() {
        let f = fixture();
        let service = BackupService::default();
        let (old, _) = make_left_behind(&f, &service);
        // The status and the move both read names, so a half-synced backup (no manifest) is
        // not silently dropped from the count and then forgotten.
        let broken = list(&old).remove(0).name;
        fs::remove_file(old.join(&broken).join(snapshot::MANIFEST_FILE)).unwrap();
        // A plain file named like a backup is not a backup.
        fs::write(old.join("backup-20261101-000000-000"), b"x").unwrap();

        let notice = service
            .status(&f.db, &f.paths)
            .unwrap()
            .left_behind
            .unwrap();
        assert_eq!(notice.backups, 2);

        let outcome = service.relocate_left_behind(&f.db, &f.paths, true).unwrap();
        assert_eq!((outcome.report.removed, outcome.report.failed), (1, 1));
        let left = outcome.status.left_behind.unwrap();
        assert_eq!(left.backups, 1, "the offer stays for the one that failed");
        assert!(old.join(&broken).is_dir(), "the original is untouched");
    }

    /// Work that blocks until the returned sender is dropped or used: a stand-in for a read
    /// stuck on a sleeping drive, released by the test and not by the clock.
    fn blocked() -> (mpsc::Sender<()>, impl FnOnce() -> u32 + Send + 'static) {
        let (release, wait) = mpsc::channel::<()>();
        (release, move || {
            let _ = wait.recv();
            7
        })
    }

    /// Waits (briefly, polling) until `done` is true.
    fn eventually(done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "the condition never became true");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn occupied(probe: &FolderProbe) -> bool {
        !lock(&probe.probes.0).is_empty()
    }

    /// Long enough that no test depends on the clock: stuck folders are made with
    /// [`FolderProbe::mark_stuck`] or work that blocks until released.
    const LONG: Duration = Duration::from_secs(60);

    fn service_with_long_probe() -> BackupService {
        BackupService {
            probe: FolderProbe::with_timeout(LONG),
            ..BackupService::default()
        }
    }

    #[test]
    fn the_probe_gives_up_on_work_that_never_answers() {
        // Only the give-up is timed, and it can't fail by being slow: the work never ends.
        let probe = FolderProbe::with_timeout(Duration::from_millis(50));
        let (release, work) = blocked();
        assert_eq!(probe.run(Path::new("/a"), work), None);
        // It is now marked stuck, and a later call on the same folder answers at once.
        assert!(
            lock(&probe.probes.0)
                .get(Path::new("/a"))
                .is_some_and(|probe| probe.stuck)
        );
        drop(release);
    }

    #[test]
    fn a_stuck_folder_answers_at_once_and_recovers_when_its_work_ends() {
        let probe = FolderProbe::with_timeout(LONG);
        let path = Path::new("/a");
        assert_eq!(probe.run(path, || 1), Some(1));
        assert!(!occupied(&probe));

        probe.mark_stuck(path);
        // With a 60 s timeout, answering at all (instead of waiting) proves it is immediate.
        assert_eq!(probe.run(path, || 2), None);

        // A stuck entry is freed by its own worker; emulate that and check it works again.
        lock(&probe.probes.0).remove(path);
        assert_eq!(probe.run(path, || 3), Some(3));
    }

    #[test]
    fn a_probe_stuck_on_one_folder_does_not_stop_another_folder() {
        let probe = FolderProbe::with_timeout(LONG);
        probe.mark_stuck(Path::new("/offline"));

        assert_eq!(probe.run(Path::new("/healthy"), || 5), Some(5));
        assert_eq!(probe.run(Path::new("/offline"), || 6), None);
    }

    #[test]
    fn a_concurrent_call_waits_for_a_healthy_probe_and_then_runs() {
        let probe = Arc::new(FolderProbe::with_timeout(Duration::from_secs(10)));
        let (release, work) = blocked();
        let first = {
            let probe = probe.clone();
            std::thread::spawn(move || probe.run(Path::new("/a"), work))
        };
        eventually(|| occupied(&probe));
        let second = {
            let probe = probe.clone();
            std::thread::spawn(move || probe.run(Path::new("/a"), || 2))
        };
        // The second one is waiting; letting the first finish lets it through.
        std::thread::sleep(Duration::from_millis(50));
        drop(release);
        assert_eq!(first.join().unwrap(), Some(7));
        assert_eq!(second.join().unwrap(), Some(2));
    }

    #[test]
    fn a_panicking_probe_does_not_disable_the_next_one() {
        let probe = FolderProbe::with_timeout(Duration::from_secs(5));
        let path = Path::new("/a");
        assert_eq!(probe.run(path, || -> u32 { panic!("boom") }), None);
        eventually(|| !occupied(&probe));
        assert_eq!(probe.run(path, || 4), Some(4));
    }

    #[test]
    fn the_same_folder_is_answered_without_looking_at_the_disk() {
        let probe = FolderProbe::with_timeout(LONG);
        probe.mark_stuck(Path::new("/offline"));
        assert!(matches!(
            probe.inspect(Path::new("/offline"), Path::new("/offline")),
            Some(OldFolder::Current)
        ));
    }

    #[test]
    fn a_chosen_folder_that_does_not_answer_keeps_both_folder_settings() {
        let f = fixture();
        let service = service_with_long_probe();
        let (old, current) = make_left_behind(&f, &service);
        let chosen = f._dir.path().join("offline");
        service.probe.mark_stuck(&candidate_key(&chosen));

        let error = service
            .change_folder(&f.db, &f.paths, Some(&chosen))
            .unwrap_err();

        assert!(error.to_string().contains("não está acessível"), "{error}");
        let settings =
            f.db.with(|conn| Ok((folder(conn, &f.paths)?, get_setting(conn, KEY_LEFT_BEHIND)?)))
                .unwrap();
        assert_eq!(settings, (current, Some(old.display().to_string())));
    }

    #[test]
    fn a_stalled_candidate_check_leaves_the_current_folder_readable() {
        let f = fixture();
        let mut service = service_with_long_probe();
        let folder = f._dir.path().join("chosen");
        service
            .change_folder(&f.db, &f.paths, Some(&folder))
            .unwrap();

        // Hold the write check past its deadline and until both read probes finish.
        let (release, work) = blocked();
        service.probe.timeout = Duration::from_millis(50);
        let error = service
            .change_folder_with(&f.db, &f.paths, Some(&folder), move |chosen, _| {
                work();
                Ok(chosen.display().to_string())
            })
            .unwrap_err();
        assert!(error.to_string().contains("não está acessível"), "{error}");
        service.probe.timeout = LONG;

        let status = service.status(&f.db, &f.paths).unwrap();
        assert!(!status.folder_unavailable);
        service
            .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 10:00:00 UTC))
            .unwrap()
            .expect("the daily backup is due");
        drop(release);
    }

    #[test]
    fn a_backup_can_finish_while_the_chosen_folder_is_being_checked() {
        let f = fixture();
        let service = service_with_long_probe();
        let old = default_folder(&f.paths);
        let chosen = f._dir.path().join("chosen");
        let (f, service) = (&f, &service);
        std::thread::scope(|scope| {
            let (checking, started) = mpsc::channel();
            let (release, wait) = mpsc::channel::<()>();
            let change = scope.spawn(|| {
                service.change_folder_with(&f.db, &f.paths, Some(&chosen), move |chosen, images| {
                    checking.send(()).unwrap();
                    let _ = wait.recv();
                    check_chosen_folder(chosen, images)
                })
            });
            started.recv_timeout(Duration::from_secs(10)).unwrap();

            let (done, backup) = mpsc::channel();
            scope.spawn(move || {
                let result = service.backup_now(&f.db, &f.paths, "2026-11-19T10:00:00.000Z");
                let _ = done.send(result);
            });
            backup
                .recv_timeout(Duration::from_secs(10))
                .expect("the backup can acquire both locks")
                .unwrap();
            release.send(()).unwrap();

            let status = change.join().unwrap().unwrap();
            let left = status.left_behind.unwrap();
            assert_eq!((left.folder, left.backups), (old.display().to_string(), 1));
        });
    }

    #[test]
    fn an_old_folder_that_does_not_answer_is_offered_as_unavailable_without_blocking() {
        let f = fixture();
        let (old, new) = make_left_behind(&f, &BackupService::default());
        let service = service_with_long_probe();
        // Reading the old folder is stuck.
        service.probe.mark_stuck(&old);

        let notice = service
            .status(&f.db, &f.paths)
            .unwrap()
            .left_behind
            .unwrap();
        assert!(notice.unavailable);
        assert_eq!(notice.folder, old.display().to_string());

        // Changing the folder while the folder being left (`new` now) doesn't answer keeps the
        // offer for it (as unavailable) instead of losing it.
        service.probe.mark_stuck(&new);
        let third = f._dir.path().join("third");
        let status = service
            .change_folder(&f.db, &f.paths, Some(&third))
            .unwrap();
        let left = status.left_behind.unwrap();
        assert!(left.unavailable);
        assert_eq!(left.folder, new.display().to_string());
        let error = service
            .relocate_left_behind(&f.db, &f.paths, true)
            .unwrap_err();
        assert!(error.to_string().contains("não está acessível"), "{error}");
    }

    #[test]
    fn a_folder_that_stops_answering_after_a_move_keeps_the_offer() {
        let f = fixture();
        let service = service_with_long_probe();
        let (old, _) = make_left_behind(&f, &service);
        // Leave one backup the move can't verify, so something remains, then make the folder
        // stop answering just before the final check.
        let broken = list(&old).remove(0).name;
        fs::remove_file(old.join(&broken).join(snapshot::MANIFEST_FILE)).unwrap();

        let outcome = service
            .relocate_with(&f.db, &f.paths, true, &|| service.probe.mark_stuck(&old))
            .unwrap();

        assert_eq!(outcome.report.failed, 1);
        let left = outcome.status.left_behind.expect("the offer stays");
        assert!(left.unavailable);
    }

    #[test]
    fn a_current_folder_that_does_not_answer_shows_as_unavailable_and_blocks_nothing() {
        let f = fixture();
        let service = service_with_long_probe();
        let folder = default_folder(&f.paths);
        let made = backup(&f, "2026-11-19T10:00:00.000Z");
        let healthy = service.status(&f.db, &f.paths).unwrap();
        assert_eq!(
            healthy.listing,
            BackupListing::Ready {
                backups: vec![made]
            }
        );

        service.probe.mark_stuck(&folder);

        let status = service.status(&f.db, &f.paths).unwrap();
        assert_eq!(status.listing, BackupListing::FolderUnavailable);
        // Nothing that takes the backup lock waits on the folder: each is refused at once.
        let now = "2026-11-19T12:00:00.000Z";
        let errors = [
            service.backup_now(&f.db, &f.paths, now).unwrap_err(),
            service
                .backup_if_due(&f.db, &f.paths, datetime!(2026-11-21 10:00:00 UTC))
                .unwrap_err(),
            service
                .restore(
                    &f.db,
                    &f.paths,
                    "backup-20261119-100000-000",
                    now,
                    || Ok(()),
                )
                .unwrap_err(),
            service
                .import(&f.db, &f.paths, Path::new("/nowhere.zip"), now, || Ok(()))
                .unwrap_err(),
        ];
        for error in errors {
            assert!(error.to_string().contains("não está acessível"), "{error}");
        }
        assert!(
            service
                .status(&f.db, &f.paths)
                .unwrap()
                .last_error
                .is_some()
        );
        assert_eq!(list(&folder).len(), 1, "nothing was written or removed");
    }

    #[test]
    fn a_backup_folder_that_does_not_exist_yet_is_reachable() {
        let f = fixture();
        let service = service_with_long_probe();
        // No backup was ever made: the default folder is missing, and that is fine.
        assert!(!default_folder(&f.paths).exists());
        assert_eq!(
            service.status(&f.db, &f.paths).unwrap().listing,
            BackupListing::Ready {
                backups: Vec::new()
            }
        );
        service
            .backup_now(&f.db, &f.paths, "2026-11-19T10:00:00.000Z")
            .unwrap();
        assert_eq!(list(&default_folder(&f.paths)).len(), 1);
    }

    #[test]
    fn a_stamp_reads_as_the_time_of_the_backup() {
        assert_eq!(
            stamp_to_iso("20261119-100203-045"),
            "2026-11-19T10:02:03.045Z"
        );
    }

    #[test]
    fn the_daily_check_reads_names_not_manifests() {
        let f = fixture();
        let service = service_with_long_probe();
        let info = backup(&f, "2026-11-19T10:00:00.000Z");
        // A manifest that can't be read (a cloud-only placeholder, a half-synced copy) must not
        // make the backup look absent, nor slow the check down.
        let folder = default_folder(&f.paths);
        fs::remove_file(folder.join(&info.name).join(snapshot::MANIFEST_FILE)).unwrap();

        let made = service
            .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 11:00:00 UTC))
            .unwrap();
        assert!(made.is_none(), "the backup is there, by its name");
        let made = service
            .backup_if_due(&f.db, &f.paths, datetime!(2026-11-20 10:00:01 UTC))
            .unwrap();
        assert!(made.is_some(), "and a day later a new one is due");
    }

    #[test]
    fn a_drive_that_is_gone_shows_as_unavailable_and_a_new_folder_does_not() {
        let f = fixture();
        let service = service_with_long_probe();
        let set = |folder: &Path| {
            f.db.with(|conn| set_setting(conn, KEY_FOLDER, &folder.display().to_string()))
                .unwrap();
        };
        // The parent (the volume) is gone: nothing to create the folder in.
        set(&f._dir.path().join("volume-gone").join("backups"));
        assert_eq!(
            service.status(&f.db, &f.paths).unwrap().listing,
            BackupListing::FolderUnavailable
        );
        assert!(
            service
                .backup_now(&f.db, &f.paths, "2026-11-19T10:00:00.000Z")
                .is_err()
        );

        // The parent exists and the folder is only missing: fine, created on first use.
        set(&f._dir.path().join("backups-novos"));
        assert_eq!(
            service.status(&f.db, &f.paths).unwrap().listing,
            BackupListing::Ready {
                backups: Vec::new()
            }
        );
    }

    #[test]
    fn a_new_folder_that_does_not_answer_stops_the_relocation_before_it_creates_anything() {
        let f = fixture();
        let service = service_with_long_probe();
        let (old, new) = make_left_behind(&f, &service);
        service.probe.mark_stuck(&new);

        let error = service
            .relocate_left_behind(&f.db, &f.paths, true)
            .unwrap_err();

        assert!(error.to_string().contains("não está acessível"), "{error}");
        assert_eq!(list(&old).len(), 2, "nothing was touched");
    }

    #[test]
    fn stuck_manifests_report_a_slow_list_while_backups_keep_working() {
        let f = fixture();
        let service = service_with_long_probe();
        let folder = default_folder(&f.paths);
        fs::create_dir_all(&folder).unwrap();
        // An earlier manifest read has already reached its deadline.
        service.probe.mark_stuck(&manifest_key(&folder));
        assert_eq!(
            service.status(&f.db, &f.paths).unwrap().listing,
            BackupListing::Slow
        );
        let daily = service
            .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 10:00:00 UTC))
            .unwrap()
            .expect("a daily backup was due");
        let manual = service
            .backup_now(&f.db, &f.paths, "2026-11-19T11:00:00.000Z")
            .unwrap();
        assert_eq!(
            service.status(&f.db, &f.paths).unwrap().listing,
            BackupListing::Slow
        );
        for info in [daily, manual] {
            snapshot::verify(&folder.join(info.name)).unwrap();
        }
    }

    #[test]
    fn a_slow_manifest_read_does_not_hold_the_folders_own_slot() {
        let service = BackupService {
            probe: FolderProbe::with_timeout(Duration::from_secs(5)),
            ..BackupService::default()
        };
        let folder = Path::new("/some/backups");
        let (release, work) = blocked();
        let key = manifest_key(folder);
        std::thread::scope(|scope| {
            let slow = scope.spawn(|| service.probe.run_for(&key, LONG, work));
            eventually(|| occupied(&service.probe));
            assert_eq!(service.probe.run(folder, || 1), Some(1));
            drop(release);
            assert_eq!(slow.join().unwrap(), Some(7));
        });
    }

    #[test]
    fn the_daily_backup_refuses_a_gone_drive_like_everything_else() {
        let f = fixture();
        let service = service_with_long_probe();
        f.db.with(|conn| {
            let gone = f._dir.path().join("volume-gone").join("backups");
            set_setting(conn, KEY_FOLDER, &gone.display().to_string())
        })
        .unwrap();

        let error = service
            .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 10:00:00 UTC))
            .unwrap_err();

        assert!(error.to_string().contains("não está acessível"), "{error}");
        assert!(
            !f._dir.path().join("volume-gone").exists(),
            "nothing recreated"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_folder_on_a_volume_that_is_not_mounted_is_unavailable() {
        assert!(matches!(
            folder_state(Path::new("/Volumes/gabaritei-no-such-volume")),
            FolderState::Unavailable
        ));
    }

    #[test]
    fn untagged_backups_from_before_tags_still_count_for_the_daily_check() {
        let f = fixture();
        let service = service_with_long_probe();
        backup(&f, "2026-11-19T10:00:00.000Z");
        as_computer("desktop");

        let made = service
            .backup_if_due(&f.db, &f.paths, datetime!(2026-11-19 11:00:00 UTC))
            .unwrap();

        assert!(
            made.is_none(),
            "the untagged one is from before tags, this computer's"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_drive_root_that_is_missing_is_unavailable_not_a_new_folder() {
        let letter = ('Q'..='Z')
            .find(|letter| !Path::new(&format!("{letter}:\\")).exists())
            .expect("an unused drive letter");
        assert!(matches!(
            folder_state(Path::new(&format!("{letter}:\\"))),
            FolderState::Unavailable
        ));
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
        assert_eq!(
            outcome.status.listing,
            BackupListing::Ready {
                backups: list(&new)
            }
        );
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
                safety: 1,
                unavailable: false,
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
        // Refused up front by the folder check (#109), so nothing is left to move into it.
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
